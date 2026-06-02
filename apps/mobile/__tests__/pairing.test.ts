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
    __clear: () => {
      store.clear();
    },
  };
});

jest.mock("expo-crypto", () => ({
  randomUUID: jest.fn(() => "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"),
}));

const mockSharedSecret = new Uint8Array(32).fill(9);

jest.mock("../src/crypto/identity", () => ({
  ensureIdentity: jest.fn(async () => "sha256:mobile"),
  getDevicePubkey: jest.fn(() => new Uint8Array(32).fill(7)),
  derive_x25519: jest.fn(async (_peerPubKey: Uint8Array) => mockSharedSecret),
}));

import { derive_x25519 } from "../src/crypto/identity";
import {
  parsePairingPayload,
  validationErrorMessage,
  validatePairingPayload,
} from "../src/pairing/payload";
import { completePairing, deriveSyncKey } from "../src/pairing/handshake";
import {
  clearMobileDeviceUuid,
  ensureMobileDeviceUuid,
  getMobileDeviceUuid,
} from "../src/pairing/device";
import {
  clearPairingState,
  persistPairingState,
  restorePairingState,
} from "../src/spine/session";
import { startPairingFlow } from "../src/pairing";
import { useAppStore } from "../src/store";

function base64UrlNoPad(bytes: Uint8Array): string {
  return Buffer.from(bytes)
    .toString("base64")
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replaceAll("=", "");
}

const validPubkey = Uint8Array.from(Buffer.from(
  "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c",
  "hex",
));
const validPubkeyB64 = base64UrlNoPad(validPubkey);
const validFingerprint =
  "sha256:34750f98bd59fcfc946da45aaabe933be154a4b5094e1c4abf42866505f3c97e";
const sessionId = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const deviceUuid = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

function payload(overrides: Record<string, unknown> = {}) {
  return {
    v: 1,
    kind: "syncmind-pairing",
    session_id: sessionId,
    spine_url: "https://spine.example.com:8443",
    ca_fingerprint: null,
    pairing_token: "opaque-token",
    expires_at: new Date(Date.now() + 5 * 60_000).toISOString(),
    device_a_pubkey: validPubkeyB64,
    device_a_fingerprint: validFingerprint,
    ...overrides,
  };
}

beforeEach(async () => {
  jest.clearAllMocks();
  (SecureStore as unknown as { __clear: () => void }).__clear();
  await clearPairingState();
  useAppStore.getState().reset();
});

describe("parsePairingPayload", () => {
  it("parses a valid raw JSON payload", () => {
    const parsed = parsePairingPayload(`  ${JSON.stringify(payload())}  `);
    expect(parsed.session_id).toBe(sessionId);
    expect(parsed.device_a_pubkey).toBe(validPubkeyB64);
  });

  it("throws a typed error for malformed JSON", () => {
    expect(() => parsePairingPayload("not json")).toThrow(
      expect.objectContaining({ code: "malformed_json" }),
    );
  });

  it("throws a desktop-upgrade error when session_id is missing", () => {
    const { session_id: _sessionId, ...withoutSession } = payload();
    expect(() => parsePairingPayload(JSON.stringify(withoutSession))).toThrow(
      expect.objectContaining({ code: "desktop_upgrade_required" }),
    );
  });

  it("throws a missing field error when required fields are absent", () => {
    const { spine_url: _spineUrl, ...withoutUrl } = payload();
    expect(() => parsePairingPayload(JSON.stringify(withoutUrl))).toThrow(
      expect.objectContaining({ code: "missing_field" }),
    );
  });

  it("throws wrong_kind for the wrong payload kind", () => {
    expect(() =>
      parsePairingPayload(JSON.stringify(payload({ kind: "other" }))),
    ).toThrow(expect.objectContaining({ code: "wrong_kind" }));
  });
});

describe("validatePairingPayload", () => {
  it("accepts an expired-at value inside the 60 second clock skew", () => {
    const parsed = parsePairingPayload(
      JSON.stringify(payload({ expires_at: new Date(Date.now() - 59_000).toISOString() })),
    );
    expect(validatePairingPayload(parsed)).toBeNull();
  });

  it("rejects an expired payload beyond clock skew", () => {
    const parsed = parsePairingPayload(
      JSON.stringify(payload({ expires_at: new Date(Date.now() - 61_000).toISOString() })),
    );
    expect(validatePairingPayload(parsed)?.code).toBe("expired");
  });

  it("rejects unsupported schema versions", () => {
    const parsed = parsePairingPayload(JSON.stringify(payload({ v: 2 })));
    expect(validatePairingPayload(parsed)?.code).toBe("unsupported_version");
  });

  it("rejects invalid UUIDv4 session ids", () => {
    const parsed = parsePairingPayload(JSON.stringify(payload({ session_id: "not-a-uuid" })));
    expect(validatePairingPayload(parsed)?.code).toBe("invalid_session_id");
  });

  it("rejects fingerprint mismatches", () => {
    const parsed = parsePairingPayload(
      JSON.stringify(payload({ device_a_fingerprint: `sha256:${"00".repeat(32)}` })),
    );
    expect(validatePairingPayload(parsed)?.code).toBe("fingerprint_mismatch");
  });

  it("rejects http spine_url in production", () => {
    const parsed = parsePairingPayload(
      JSON.stringify(payload({ spine_url: "http://192.168.1.10:8080" })),
    );
    expect(validatePairingPayload(parsed, { allowHttp: false })?.code).toBe("insecure_url");
  });

  it("accepts http spine_url in dev mode", () => {
    const parsed = parsePairingPayload(
      JSON.stringify(payload({ spine_url: "http://192.168.1.10:8080" })),
    );
    expect(validatePairingPayload(parsed, { allowHttp: true })).toBeNull();
  });

  it("maps validation errors to user-readable messages", () => {
    expect(validationErrorMessage({ code: "expired" })).toBe(
      "QR code expired - please generate a new one from the desktop Devices panel",
    );
  });
});

describe("device UUID", () => {
  it("generates a stable UUID once and stores it", async () => {
    await expect(ensureMobileDeviceUuid()).resolves.toBe(deviceUuid);
    await expect(ensureMobileDeviceUuid()).resolves.toBe(deviceUuid);
    expect(SecureStore.setItemAsync).toHaveBeenCalledTimes(1);
    await expect(getMobileDeviceUuid()).resolves.toBe(deviceUuid);
  });

  it("clears the stored UUID", async () => {
    await ensureMobileDeviceUuid();
    await clearMobileDeviceUuid();
    await expect(getMobileDeviceUuid()).resolves.toBeNull();
  });
});

describe("completePairing", () => {
  it("posts session_id, device_uuid, responder_pubkey, and mobile device_type", async () => {
    const fetchMock = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({
        status: "completed",
        session_id: sessionId,
        initiator_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        responder_id: deviceUuid,
        initiator_pubkey: validPubkeyB64,
      }),
    }));
    global.fetch = fetchMock as unknown as typeof fetch;
    const parsed = parsePairingPayload(JSON.stringify(payload()));
    const responderPubkey = base64UrlNoPad(new Uint8Array(32).fill(7));

    await completePairing(parsed, deviceUuid, responderPubkey);

    const [, init] = fetchMock.mock.calls[0];
    const body = JSON.parse(init.body as string);
    expect(fetchMock).toHaveBeenCalledWith(
      "https://spine.example.com:8443/v1/pairing/complete",
      expect.objectContaining({ method: "POST" }),
    );
    expect(body).toEqual({
      session_id: sessionId,
      device_uuid: deviceUuid,
      responder_pubkey: responderPubkey,
      device_type: "mobile",
    });
    expect(body.x25519_ephemeral_pubkey).toBeUndefined();
  });

  it("reports a readable error when the identity key is already bound to another device UUID", async () => {
    global.fetch = jest.fn(async () => ({
      ok: false,
      status: 409,
      json: async () => ({
        code: "FINGERPRINT_CONFLICT",
        message: "identity key is already bound to device_uuid f57377ee-93a5-4f09-baa0-d26c0901e558",
      }),
    })) as unknown as typeof fetch;
    const parsed = parsePairingPayload(JSON.stringify(payload()));
    const responderPubkey = base64UrlNoPad(new Uint8Array(32).fill(7));

    await expect(completePairing(parsed, deviceUuid, responderPubkey)).rejects.toThrow(
      "This mobile identity is already registered under another device ID - reset device identity before pairing again",
    );
  });
});

describe("deriveSyncKey", () => {
  it("produces a deterministic 32-byte key with the syncmind HKDF parameters", async () => {
    const key1 = await deriveSyncKey(validPubkey, sessionId);
    const key2 = await deriveSyncKey(validPubkey, sessionId);
    expect(key1).toEqual(key2);
    expect(key1).toHaveLength(32);
    expect(Buffer.from(key1).toString("hex")).toBe(
      "ffcbf28beb46041b5e697d1effd69c0c2835d5fabfb9e5978bc3a68c3284a003",
    );
    expect(derive_x25519).toHaveBeenCalledWith(validPubkey);
  });
});

describe("pairing state persistence", () => {
  const state = {
    selfDeviceUuid: deviceUuid,
    syncKey: new Uint8Array(32).fill(3),
    pairedPeerFingerprint: validFingerprint,
    pairedPeerDeviceId: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
    pairedPeerDeviceType: "desktop" as const,
    pairedAt: "2026-05-31T00:00:00.000Z",
    spineUrl: "https://spine.example.com:8443",
    caFingerprint: null,
    lastSeenAt: null,
  };

  it("round-trips required state through SecureStore", async () => {
    await persistPairingState(state);
    await expect(restorePairingState()).resolves.toEqual(state);
  });

  it("returns null when required state is missing", async () => {
    await persistPairingState(state);
    await SecureStore.deleteItemAsync("syncmind.pairing.sync_key");
    await expect(restorePairingState()).resolves.toBeNull();
  });

  it("removes all pairing state keys", async () => {
    await persistPairingState(state);
    await clearPairingState();
    await expect(restorePairingState()).resolves.toBeNull();
    expect(SecureStore.deleteItemAsync).toHaveBeenCalledWith(
      "syncmind.pairing.paired_peer_fingerprint",
    );
  });
});

describe("startPairingFlow", () => {
  it("completes pairing, persists state, and marks the store connected", async () => {
    global.fetch = jest.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({
        status: "completed",
        session_id: sessionId,
        initiator_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        responder_id: deviceUuid,
        initiator_pubkey: validPubkeyB64,
      }),
    })) as unknown as typeof fetch;
    const parsed = parsePairingPayload(JSON.stringify(payload()));

    await startPairingFlow(parsed);

    const restored = await restorePairingState();
    expect(restored?.selfDeviceUuid).toBe(deviceUuid);
    expect(restored?.pairedPeerFingerprint).toBe(validFingerprint);
    expect(restored?.pairedPeerDeviceId).toBe("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
    expect(restored?.spineUrl).toBe("https://spine.example.com:8443");
    expect(useAppStore.getState()).toEqual(
      expect.objectContaining({
        isPaired: true,
        peerDeviceFingerprint: validFingerprint,
        connectionStatus: "connected",
        isFirstPairing: true,
      }),
    );
  });

  it("recovers a lost mobile device UUID from a fingerprint conflict and retries pairing", async () => {
    const existingDeviceUuid = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
    const fetchMock = jest
      .fn()
      .mockResolvedValueOnce({
        ok: false,
        status: 409,
        json: async () => ({
          code: "FINGERPRINT_CONFLICT",
          existing_device_uuid: existingDeviceUuid,
        }),
      })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({
          status: "completed",
          session_id: sessionId,
          initiator_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
          responder_id: existingDeviceUuid,
          initiator_pubkey: validPubkeyB64,
        }),
      });
    global.fetch = fetchMock as unknown as typeof fetch;
    const parsed = parsePairingPayload(JSON.stringify(payload()));

    await startPairingFlow(parsed);

    const [, retryInit] = fetchMock.mock.calls[1];
    expect(JSON.parse(retryInit.body as string).device_uuid).toBe(existingDeviceUuid);
    const restored = await restorePairingState();
    expect(restored?.selfDeviceUuid).toBe(existingDeviceUuid);
    await expect(ensureMobileDeviceUuid()).resolves.toBe(existingDeviceUuid);
  });
});
