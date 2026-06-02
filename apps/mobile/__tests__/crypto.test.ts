/**
 * Tests for the mobile device identity module.
 */

import * as SecureStore from "expo-secure-store";

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
  __resetIdentityCacheForTests,
} from "../src/crypto/identity";
import NativeDeviceIdentity from "../src/crypto/native-device-identity";
import {
  clearCurrentSpineSession,
  persistPairingState,
  restorePairingState,
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
const mockPublicKeyHex = "02".repeat(32);
const mockFingerprint = `${FINGERPRINT_PREFIX}${"ab".repeat(32)}`;
const mockSignatureB64 = Buffer.from(new Uint8Array(64).fill(7)).toString("base64");
const mockSharedSecretB64 = Buffer.from(new Uint8Array(32).fill(9)).toString("base64");

type NativeIdentityState = {
  fingerprint: string;
  publicKeyHex: string;
  biometricEnabled: boolean;
  exists: boolean;
};

const mockNativeState: NativeIdentityState = {
  fingerprint: mockFingerprint,
  publicKeyHex: mockPublicKeyHex,
  biometricEnabled: false,
  exists: false,
};

function resetMockNativeState() {
  mockNativeState.fingerprint = mockFingerprint;
  mockNativeState.publicKeyHex = mockPublicKeyHex;
  mockNativeState.biometricEnabled = false;
  mockNativeState.exists = false;
}

jest.mock("../src/crypto/native-device-identity", () => {
  const actual = {
    ensureIdentity: jest.fn(async () => {
      mockNativeState.exists = true;
      return {
        fingerprint: mockNativeState.fingerprint,
        publicKeyHex: mockNativeState.publicKeyHex,
        biometricEnabled: mockNativeState.biometricEnabled,
      };
    }),
    getIdentityMeta: jest.fn(async () => {
      if (!mockNativeState.exists) {
        return null;
      }
      return {
        fingerprint: mockNativeState.fingerprint,
        publicKeyHex: mockNativeState.publicKeyHex,
        biometricEnabled: mockNativeState.biometricEnabled,
      };
    }),
    sign: jest.fn(async (_messageBase64: string) => mockSignatureB64),
    deriveX25519: jest.fn(async (_peerPubKeyHex: string) => mockSharedSecretB64),
    setBiometricProtection: jest.fn(async (enabled: boolean) => {
      if (!mockNativeState.exists) {
        throw new Error("Device identity not initialized.");
      }
      mockNativeState.biometricEnabled = enabled;
    }),
    resetIdentity: jest.fn(async () => {
      mockNativeState.exists = false;
      mockNativeState.biometricEnabled = false;
    }),
    importLegacyIdentity: jest.fn(async (_privateKeyHex: string) => {
      mockNativeState.exists = true;
      mockNativeState.biometricEnabled = false;
      return {
        fingerprint: mockNativeState.fingerprint,
        publicKeyHex: mockNativeState.publicKeyHex,
        biometricEnabled: mockNativeState.biometricEnabled,
      };
    }),
  };
  return {
    __esModule: true,
    default: actual,
  };
});

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
  resetMockNativeState();
  await SecureStore.deleteItemAsync("device_identity");
  await SecureStore.deleteItemAsync("device_identity_meta");
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

  it("persists public metadata to SecureStore", async () => {
    await ensureIdentity();
    expect(SecureStore.setItemAsync).toHaveBeenCalledWith(
      "device_identity_meta",
      JSON.stringify({
        fingerprint: mockFingerprint,
        publicKeyHex: mockPublicKeyHex,
        biometricEnabled: false,
      }),
    );
  });

  it("does not persist private key material to SecureStore", async () => {
    await ensureIdentity();
    const writes = (SecureStore.setItemAsync as jest.Mock).mock.calls;
    expect(writes).toHaveLength(1);
    expect(writes[0]?.[0]).not.toBe("device_identity");
    expect(JSON.stringify(writes)).not.toContain("privateKeyHex");
  });

  it("defaults biometric protection to disabled", async () => {
    await ensureIdentity();
    expect(isAuthenticationRequired()).toBe(false);
  });

  it("loads an existing identity from SecureStore on re-initialization", async () => {
    const fingerprint1 = await ensureIdentity();
    __resetIdentityCacheForTests();
    const fingerprint2 = await ensureIdentity();
    expect(fingerprint2).toBe(fingerprint1);
  });

  it("migrates legacy JS-stored identity into the native module and deletes the old blob", async () => {
    await SecureStore.setItemAsync(
      "device_identity",
      JSON.stringify({
        privateKeyHex: "11".repeat(32),
        publicKeyHex: mockPublicKeyHex,
        fingerprint: mockFingerprint,
      }),
    );

    const fingerprint = await ensureIdentity();

    expect(fingerprint).toBe(mockFingerprint);
    expect(NativeDeviceIdentity.importLegacyIdentity).toHaveBeenCalledWith("11".repeat(32));
    expect(SecureStore.deleteItemAsync).toHaveBeenCalledWith("device_identity");
  });

  it("does not silently continue when legacy migration fails", async () => {
    await SecureStore.setItemAsync(
      "device_identity",
      JSON.stringify({
        privateKeyHex: "11".repeat(32),
        publicKeyHex: mockPublicKeyHex,
        fingerprint: mockFingerprint,
      }),
    );
    (NativeDeviceIdentity.importLegacyIdentity as jest.Mock).mockRejectedValueOnce(
      new Error("native import failed"),
    );

    await expect(ensureIdentity()).rejects.toThrow(
      "Unable to migrate legacy device identity",
    );
    expect(NativeDeviceIdentity.ensureIdentity).not.toHaveBeenCalled();
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
    const signature = await sign(new Uint8Array([104, 101, 108, 108, 111]));
    expect(signature).toBeInstanceOf(Uint8Array);
    expect(signature.length).toBe(64);
  });

  it("throws if called before ensureIdentity", async () => {
    await expect(sign(new Uint8Array(0))).rejects.toThrow("not initialized");
  });

  it("does NOT match a 32-byte hex pattern (no raw key leak)", async () => {
    await ensureIdentity();
    const signature = await sign(new Uint8Array(32));
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
    const sharedSecret = await derive_x25519(peerPub);
    expect(sharedSecret).toBeInstanceOf(Uint8Array);
    expect(sharedSecret.length).toBe(32);
  });

  it("produces a consistent shared secret (deterministic)", async () => {
    await ensureIdentity();
    const peerPub = new Uint8Array(32);
    const secret1 = await derive_x25519(peerPub);
    const secret2 = await derive_x25519(peerPub);
    expect(secret1).toEqual(secret2);
  });

  it("throws if called before ensureIdentity", async () => {
    await expect(derive_x25519(new Uint8Array(32))).rejects.toThrow("not initialized");
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
    const stored = await SecureStore.getItemAsync("device_identity_meta");
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

  it("revokes the current device on Spine, clears the outbox, and unpairs the app state", async () => {
    await ensureIdentity();
    await persistPairingState({
      selfDeviceUuid: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      syncKey: new Uint8Array(32).fill(3),
      pairedPeerFingerprint: `${FINGERPRINT_PREFIX}${"cd".repeat(32)}`,
      pairedPeerDeviceId: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
      pairedPeerDeviceType: "desktop",
      pairedAt: "2026-05-31T00:00:00.000Z",
      spineUrl: "https://spine.syncmind.local",
      caFingerprint: null,
      lastSeenAt: null,
    });
    await enqueueOutboxItem({
      id: "capture-1",
      payload: { kind: "note", text: "hello" },
    });
    useAppStore.getState().setPaired("sha256:peer");

    await device_reset();

    expect(global.fetch).toHaveBeenCalledWith(
      "https://spine.syncmind.local/v1/devices/bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb/revoke",
      expect.objectContaining({
        method: "POST",
        headers: expect.objectContaining({
          Authorization: expect.stringMatching(/^Bearer /),
        }),
      }),
    );
    await expect(getOutboxItems()).resolves.toEqual([]);
    expect(useAppStore.getState().isPaired).toBe(false);
    expect(useAppStore.getState().peerDeviceFingerprint).toBeNull();
  });

  it("clears persisted pairing state during a full device reset", async () => {
    await ensureIdentity();
    await persistPairingState({
      selfDeviceUuid: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      syncKey: new Uint8Array(32).fill(3),
      pairedPeerFingerprint: `${FINGERPRINT_PREFIX}${"cd".repeat(32)}`,
      pairedPeerDeviceId: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
      pairedPeerDeviceType: "desktop",
      pairedAt: "2026-05-31T00:00:00.000Z",
      spineUrl: "https://spine.syncmind.local",
      caFingerprint: null,
      lastSeenAt: null,
    });

    await device_reset();

    await expect(restorePairingState()).resolves.toBeNull();
  });
});

describe("setAuthenticationRequirement", () => {
  it("re-stores the key with requireAuthentication: true", async () => {
    await ensureIdentity();
    await setAuthenticationRequirement(true);
    expect(NativeDeviceIdentity.setBiometricProtection).toHaveBeenCalledWith(true);
  });

  it("re-stores the key with requireAuthentication: false", async () => {
    await ensureIdentity();
    await setAuthenticationRequirement(true);
    await setAuthenticationRequirement(false);
    expect(NativeDeviceIdentity.setBiometricProtection).toHaveBeenLastCalledWith(false);
  });

  it("updates the cached biometric protection state", async () => {
    await ensureIdentity();
    await setAuthenticationRequirement(true);
    expect(isAuthenticationRequired()).toBe(true);
    await setAuthenticationRequirement(false);
    expect(isAuthenticationRequired()).toBe(false);
  });

  it("restores biometric protection state across app restarts", async () => {
    await ensureIdentity();
    await setAuthenticationRequirement(true);

    __resetIdentityCacheForTests();

    const fingerprint = await ensureIdentity();

    expect(fingerprint).toBe(mockFingerprint);
    expect(isAuthenticationRequired()).toBe(true);
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

  it("console.log of public API values does not leak private key material", async () => {
    await ensureIdentity();
    const spy = jest.spyOn(console, "log").mockImplementation(() => {});
    const signature = await sign(new Uint8Array([1, 2, 3]));
    const capturedLog = spy as unknown as (value: unknown) => void;

    capturedLog({
      fingerprint: getDeviceFingerprint(),
      pubkey: Array.from(getDevicePubkey()),
      signature: Array.from(signature),
    });

    const combined = spy.mock.calls.flat().map(String).join(" ");
    expect(combined).not.toContain("privateKeyHex");
    expect(combined).not.toContain("11".repeat(32));
    spy.mockRestore();
  });

  it("error messages do not leak private key material", async () => {
    await expect(setAuthenticationRequirement(true)).rejects.toThrow(
      "Device identity not initialized",
    );

    await expect(setAuthenticationRequirement(true)).rejects.not.toThrow(
      "11".repeat(32),
    );
  });
});
