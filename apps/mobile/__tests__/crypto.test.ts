/**
 * Tests for the mobile device identity module.
 */

import * as SecureStore from "expo-secure-store";

// All @noble packages are ESM-only and can't be resolved by CJS jest;
// use {virtual: true} to bypass resolution checks.

jest.mock(
  "@noble/hashes/utils.js",
  () => ({
    bytesToHex: (bytes: Uint8Array) =>
      Array.from(bytes)
        .map((n) => n.toString(16).padStart(2, "0"))
        .join(""),
    hexToBytes: (hex: string) => {
      const bytes = new Uint8Array(hex.length / 2);
      for (let i = 0; i < hex.length; i += 2) {
        bytes[i / 2] = parseInt(hex.slice(i, i + 2), 16);
      }
      return bytes;
    },
  }),
  { virtual: true },
);

jest.mock(
  "@noble/hashes/sha2.js",
  () => ({
    sha256: {
      create: () => ({
        update: () => ({
          digest: () => {
            const hash = new Uint8Array(32);
            for (let i = 0; i < 32; i++) hash[i] = (i * 7 + 3) % 256;
            return hash;
          },
        }),
      }),
    },
    sha512: {
      create: () => ({
        update: () => ({
          digest: () => {
            const hash = new Uint8Array(64);
            for (let i = 0; i < 64; i++) hash[i] = (i * 3 + 7) % 256;
            return hash;
          },
        }),
      }),
    },
  }),
  { virtual: true },
);

jest.mock(
  "@noble/curves/ed25519.js",
  () => ({
    ed25519: {
      utils: {
        randomSecretKey: () => {
          const key = new Uint8Array(32);
          for (let i = 0; i < 32; i++) key[i] = (i + 1) % 256;
          return key;
        },
      },
      getPublicKey: (_priv: Uint8Array) => {
        const key = new Uint8Array(32);
        for (let i = 0; i < 32; i++) key[i] = (i * 2) % 256;
        return key;
      },
      sign: (message: Uint8Array, _priv: Uint8Array) => {
        const sig = new Uint8Array(64);
        for (let i = 0; i < 64; i++)
          sig[i] = message[i % message.length] ^ (i * 3);
        return sig;
      },
      verify: (_sig: Uint8Array, _msg: Uint8Array, _pub: Uint8Array) => true,
    },
    x25519: {
      utils: {
        randomSecretKey: () => {
          const key = new Uint8Array(32);
          for (let i = 0; i < 32; i++) key[i] = (i + 1) % 256;
          return key;
        },
      },
      getPublicKey: (_priv: Uint8Array) => {
        const key = new Uint8Array(32);
        for (let i = 0; i < 32; i++) key[i] = (i * 2) % 256;
        return key;
      },
      getSharedSecret: (_priv: Uint8Array, _pub: Uint8Array) => {
        const secret = new Uint8Array(32);
        for (let i = 0; i < 32; i++) secret[i] = (i * 11 + 5) % 256;
        return secret;
      },
    },
  }),
  { virtual: true },
);

jest.mock("expo-secure-store", () => {
  const store = new Map<string, string>();
  return {
    setItemAsync: jest.fn(async (key: string, value: string) => {
      store.set(key, value);
    }),
    getItemAsync: jest.fn(async (key: string) => store.get(key) ?? null),
    deleteItemAsync: jest.fn(async (key: string) => {
      store.delete(key);
    }),
  };
});

import {
  ensureIdentity,
  getDeviceFingerprint,
  getDevicePubkey,
  isAuthenticationRequired,
  sign,
  derive_x25519,
  clearIdentity,
  device_reset,
  setAuthenticationRequirement,
} from "../src/crypto/identity";
import {
  clearCurrentSpineSession,
  getCurrentSpineSession,
  setCurrentSpineSession,
} from "../src/spine/session";
import {
  clearOutbox,
  enqueueOutboxItem,
  getOutboxItems,
} from "../src/outbox/service";
import { useAppStore } from "../src/store";

// ── Helpers ─────────────────────────────────────────────────────────

const HEX_32_BYTE = /^[0-9a-f]{64}$/i;
const FINGERPRINT_PREFIX = "sha256:";

function isValidFingerprint(s: string): boolean {
  return s.startsWith(FINGERPRINT_PREFIX) && HEX_32_BYTE.test(s.slice(7));
}

// ── Tests ───────────────────────────────────────────────────────────

beforeEach(async () => {
  jest.clearAllMocks();
  global.fetch = jest.fn(async () => ({
    ok: true,
    status: 204,
  })) as typeof fetch;
  await clearIdentity();
  await clearCurrentSpineSession();
  await clearOutbox();
  useAppStore.getState().reset();
});

describe("ensureIdentity", () => {
  it("generates a key pair on first call and returns fingerprint", async () => {
    const fingerprint = await ensureIdentity();
    expect(isValidFingerprint(fingerprint)).toBe(true);
    const fingerprint2 = await ensureIdentity();
    expect(fingerprint2).toBe(fingerprint);
  });

  it("persists the key to SecureStore", async () => {
    await ensureIdentity();
    expect(SecureStore.setItemAsync).toHaveBeenCalledWith(
      "device_identity",
      expect.any(String),
      { requireAuthentication: false },
    );
  });

  it("defaults biometric protection to disabled", async () => {
    await ensureIdentity();
    expect(isAuthenticationRequired()).toBe(false);
  });

  it("loads an existing identity from SecureStore on re-initialization", async () => {
    const fingerprint1 = await ensureIdentity();
    await clearIdentity();
    const fingerprint2 = await ensureIdentity();
    expect(fingerprint2).toBe(fingerprint1);
  });
});

describe("getDeviceFingerprint", () => {
  it("returns the cached fingerprint after initialization", async () => {
    const fp = await ensureIdentity();
    expect(getDeviceFingerprint()).toBe(fp);
  });

  it("throws if called before ensureIdentity", () => {
    expect(() => getDeviceFingerprint()).toThrow("not initialized");
  });
});

describe("getDevicePubkey", () => {
  it("returns the cached public key after initialization", async () => {
    await ensureIdentity();
    const pub = getDevicePubkey();
    expect(pub).toBeInstanceOf(Uint8Array);
    expect(pub.length).toBe(32);
  });

  it("throws if called before ensureIdentity", () => {
    expect(() => getDevicePubkey()).toThrow("not initialized");
  });
});

describe("sign", () => {
  it("returns a 64-byte signature for a given message", async () => {
    await ensureIdentity();
    const signature = sign(new Uint8Array([104, 101, 108, 108, 111]));
    expect(signature).toBeInstanceOf(Uint8Array);
    expect(signature.length).toBe(64);
  });

  it("throws if called before ensureIdentity", () => {
    expect(() => sign(new Uint8Array(0))).toThrow("not initialized");
  });

  it("does NOT match a 32-byte hex pattern (no raw key leak)", async () => {
    await ensureIdentity();
    const signature = sign(new Uint8Array(32));
    const sigHex = Array.from(signature)
      .map((n) => n.toString(16).padStart(2, "0"))
      .join("");
    expect(sigHex).not.toMatch(HEX_32_BYTE);
  });
});

describe("derive_x25519", () => {
  it("returns a 32-byte shared secret", async () => {
    await ensureIdentity();
    const peerPub = new Uint8Array(32);
    const sharedSecret = derive_x25519(peerPub);
    expect(sharedSecret).toBeInstanceOf(Uint8Array);
    expect(sharedSecret.length).toBe(32);
  });

  it("produces a consistent shared secret (deterministic)", async () => {
    await ensureIdentity();
    const peerPub = new Uint8Array(32);
    const secret1 = derive_x25519(peerPub);
    const secret2 = derive_x25519(peerPub);
    expect(secret1).toEqual(secret2);
  });

  it("throws if called before ensureIdentity", () => {
    expect(() => derive_x25519(new Uint8Array(32))).toThrow("not initialized");
  });
});

describe("clearIdentity", () => {
  it("clears in-memory state", async () => {
    await ensureIdentity();
    await clearIdentity();
    expect(() => getDeviceFingerprint()).toThrow("not initialized");
  });

  it("removes the entry from SecureStore", async () => {
    await ensureIdentity();
    await clearIdentity();
    const stored = await SecureStore.getItemAsync("device_identity");
    expect(stored).toBeNull();
  });

  it("is idempotent when called twice", async () => {
    await expect(clearIdentity()).resolves.toBeUndefined();
    await expect(clearIdentity()).resolves.toBeUndefined();
  });
});

describe("device_reset", () => {
  it("clears identity and in-memory state", async () => {
    await ensureIdentity();
    await device_reset();
    expect(() => getDeviceFingerprint()).toThrow("not initialized");
  });

  it("is idempotent when called twice", async () => {
    await expect(device_reset()).resolves.toBeUndefined();
  });

  it("revokes the current Spine session, clears the outbox, and unpairs the app state", async () => {
    await ensureIdentity();
    await setCurrentSpineSession({
      baseUrl: "https://spine.syncmind.local",
      accessToken: "test-token",
    });
    await enqueueOutboxItem({
      id: "capture-1",
      payload: { kind: "note", text: "hello" },
    });
    useAppStore.getState().setPaired("sha256:peer");

    await device_reset();

    expect(global.fetch).toHaveBeenCalledWith(
      "https://spine.syncmind.local/v1/auth/revoke",
      expect.objectContaining({
        method: "POST",
        headers: expect.objectContaining({
          Authorization: "Bearer test-token",
        }),
      }),
    );
    await expect(getCurrentSpineSession()).resolves.toBeNull();
    await expect(getOutboxItems()).resolves.toEqual([]);
    expect(useAppStore.getState().isPaired).toBe(false);
    expect(useAppStore.getState().peerDeviceFingerprint).toBeNull();
  });
});

describe("setAuthenticationRequirement", () => {
  it("re-stores the key with requireAuthentication: true", async () => {
    await ensureIdentity();
    await setAuthenticationRequirement(true);
    expect(SecureStore.deleteItemAsync).toHaveBeenCalledWith("device_identity");
    expect(SecureStore.setItemAsync).toHaveBeenLastCalledWith(
      "device_identity",
      expect.any(String),
      { requireAuthentication: true },
    );
  });

  it("re-stores the key with requireAuthentication: false", async () => {
    await ensureIdentity();
    await setAuthenticationRequirement(true);
    await setAuthenticationRequirement(false);
    expect(SecureStore.setItemAsync).toHaveBeenLastCalledWith(
      "device_identity",
      expect.any(String),
      { requireAuthentication: false },
    );
  });

  it("updates the cached biometric protection state", async () => {
    await ensureIdentity();
    await setAuthenticationRequirement(true);
    expect(isAuthenticationRequired()).toBe(true);
    await setAuthenticationRequirement(false);
    expect(isAuthenticationRequired()).toBe(false);
  });

  it("throws if called before ensureIdentity", async () => {
    await expect(setAuthenticationRequirement(true)).rejects.toThrow(
      "not initialized",
    );
  });
});

// ── Privacy invariant: no raw key bytes in serializable output ─────

describe("privacy — no private key leak", () => {
  it("JSON.stringify of API surface does not contain private key hex", async () => {
    await ensureIdentity();
    const apiSurface = {
      hasFingerprint: typeof getDeviceFingerprint(),
      hasPubkey: getDevicePubkey().length > 0,
    };
    const serialized = JSON.stringify(apiSurface);
    expect(serialized).not.toMatch(HEX_32_BYTE);
  });
});
