// ── Low-level mocks — apply before any module imports ───────────────────

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
  randomUUID: jest.fn(() => "cccccccc-cccc-4ccc-8ccc-cccccccccccc"),
  getRandomBytes: jest.fn((size: number) => new Uint8Array(size).fill(0x01)),
}));

jest.mock("expo-sqlite", () => ({
  openDatabaseAsync: jest.fn(async () => ({
    execAsync: jest.fn(async () => {}),
    runAsync: jest.fn(async () => ({ lastInsertRowId: 0, changes: 0 })),
    getFirstAsync: jest.fn(async () => null),
    getAllAsync: jest.fn(async () => []),
    closeAsync: jest.fn(async () => {}),
  })),
}));
jest.mock("../src/crypto/native-device-identity", () => {
  return {
    __esModule: true,
    default: {
      ensureIdentity: jest.fn(async () => ({
        publicKeyHex: "02".repeat(32),
        fingerprint: "sha256:mobile-identity-fp",
        requireAuthentication: false,
      })),
      getIdentityMeta: jest.fn(async () => ({
        publicKeyHex: "02".repeat(32),
        fingerprint: "sha256:mobile-identity-fp",
        requireAuthentication: false,
      })),
      sign: jest.fn(async (_encodedMessage: string) => {
        const sig = new Uint8Array(64).fill(0xab);
        let base64 = "";
        for (const b of sig) base64 += String.fromCharCode(b);
        return btoa(base64);
      }),
      deriveX25519: jest.fn(async (_peerPubKey: string) => {
        const shared = new Uint8Array(32).fill(9);
        let base64 = "";
        for (const b of shared) base64 += String.fromCharCode(b);
        return btoa(base64);
      }),
      setBiometricProtection: jest.fn(async (_require: boolean) => {}),
      resetIdentity: jest.fn(async () => {}),
      importLegacyIdentity: jest.fn(async () => null),
    },
  };
});

import * as SecureStore from "expo-secure-store";
import {
  fingerprintShort,
  relativeTime,
  settingsStyles,
} from "../app/(tabs)/settings";
import {
  persistPairingState,
  restorePairingState,
  clearPairingState,
  updateLastSeenAt,
  getLastSeenAt,
  getRestoredPairingState,
  __resetLastSeenThrottleForTests,
} from "../src/spine/session";
import { ensureMobileDeviceUuid } from "../src/pairing/device";
import {
  AuthPromptRequiredError,
  authenticatedFetch,
  checkCurrentDevicePairing,
  __resetDeviceJWTCacheForTests,
  UnpairedError,
} from "../src/spine/client";
import { unpair, device_reset } from "../src/crypto/identity";
import { useAppStore } from "../src/store";
import { enqueueOutboxItem, getOutboxItems } from "../src/outbox/service";
import { ensureIdentity } from "../src/crypto/identity";
import NativeDeviceIdentity from "../src/crypto/native-device-identity";

// ── Helpers ────────────────────────────────────────────────────────────

const FINGERPRINT_PREFIX = "sha256:";

function buildFingerprint(hex: string): string {
  return `${FINGERPRINT_PREFIX}${hex}`;
}

function decodeJwtPayload(token: string): Record<string, unknown> {
  const payload = token.split(".")[1];
  if (!payload) {
    throw new Error("JWT payload segment missing");
  }
  const normalized = payload.replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, "=");
  return JSON.parse(atob(padded)) as Record<string, unknown>;
}

const defaultPairingState = {
  selfDeviceUuid: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  syncKey: new Uint8Array(32).fill(3),
  pairedPeerFingerprint: buildFingerprint("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"),
  pairedPeerDeviceId: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
  pairedPeerDeviceType: "desktop" as const,
  pairedAt: "2026-05-31T00:00:00.000Z",
  spineUrl: "https://spine.syncmind.local:8443",
  caFingerprint: null,
  lastSeenAt: "2026-05-31T12:00:00.000Z",
};

beforeEach(async () => {
  (SecureStore as unknown as { __clear: () => void }).__clear();
  jest.clearAllMocks();
  useAppStore.getState().reset();
  await clearPairingState();
  __resetLastSeenThrottleForTests();
  __resetDeviceJWTCacheForTests();
  global.fetch = jest.fn(async () => new Response(null, { status: 204 })) as typeof fetch;
});

// ── fingerprintShort ───────────────────────────────────────────────────

describe("fingerprintShort", () => {
  it("truncates a full sha256 fingerprint to first 8 + … + last 4 hex chars", () => {
    const fp = buildFingerprint("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2");
    expect(fingerprintShort(fp)).toBe("sha256:a1b2c3d4…a1b2");
  });

  it("returns the input unchanged when hex part is 12 chars or fewer", () => {
    expect(fingerprintShort("sha256:abcdefghijkl")).toBe("sha256:abcdefghijkl");
  });

  it("normalizes a bare hex fingerprint by adding the sha256: prefix", () => {
    const hex = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
    expect(fingerprintShort(hex)).toBe("sha256:a1b2c3d4…a1b2");
  });
});

// ── relativeTime ────────────────────────────────────────────────────────

describe("relativeTime", () => {
  it('returns "Never" for null', () => {
    expect(relativeTime(null)).toBe("Never");
  });

  it('returns "Unknown" for invalid ISO strings', () => {
    expect(relativeTime("not-a-date")).toBe("Unknown");
  });

  it('returns "Just now" for timestamps less than 60s ago', () => {
    const recent = new Date(Date.now() - 30_000).toISOString();
    expect(relativeTime(recent)).toBe("Just now");
  });

  it("returns minutes ago", () => {
    const ts = new Date(Date.now() - 5 * 60_000).toISOString();
    expect(relativeTime(ts)).toBe("5m ago");
  });

  it("returns hours ago", () => {
    const ts = new Date(Date.now() - 3 * 3_600_000).toISOString();
    expect(relativeTime(ts)).toBe("3h ago");
  });

  it("returns days ago", () => {
    const ts = new Date(Date.now() - 5 * 86_400_000).toISOString();
    expect(relativeTime(ts)).toBe("5d ago");
  });

  it("returns months ago", () => {
    const ts = new Date(Date.now() - 45 * 86_400_000).toISOString();
    expect(relativeTime(ts)).toBe("1mo ago");
  });
});

// ── Settings layout ────────────────────────────────────────────────────

describe("Settings layout", () => {
  it("keeps the Settings content scrollable when paired details add vertical height", () => {
    expect(settingsStyles.container).toEqual(
      expect.objectContaining({ flex: 1 }),
    );
    expect(settingsStyles.scrollContent).toEqual(
      expect.objectContaining({
        paddingHorizontal: 20,
        paddingTop: 60,
        paddingBottom: 40,
      }),
    );
  });
});

// ── PersistedPairingState with lastSeenAt ───────────────────────────────

describe("PersistedPairingState with lastSeenAt", () => {
  it("persists and restores lastSeenAt", async () => {
    await persistPairingState(defaultPairingState);

    const restored = await restorePairingState();
    expect(restored).not.toBeNull();
    expect(restored!.lastSeenAt).toBe("2026-05-31T12:00:00.000Z");
  });

  it("restores lastSeenAt as null when stored as 'null'", async () => {
    await persistPairingState({ ...defaultPairingState, lastSeenAt: null });

    const restored = await restorePairingState();
    expect(restored).not.toBeNull();
    expect(restored!.lastSeenAt).toBeNull();
  });

  it("restores legacy US-041 state (no last_seen_at key) with lastSeenAt: null", async () => {
    await SecureStore.setItemAsync("syncmind.pairing.self_device_uuid", defaultPairingState.selfDeviceUuid);
    await SecureStore.setItemAsync("syncmind.pairing.sync_key", "AAAA");
    await SecureStore.setItemAsync("syncmind.pairing.paired_peer_fingerprint", defaultPairingState.pairedPeerFingerprint);
    await SecureStore.setItemAsync("syncmind.pairing.paired_peer_device_id", defaultPairingState.pairedPeerDeviceId);
    await SecureStore.setItemAsync("syncmind.pairing.paired_peer_device_type", defaultPairingState.pairedPeerDeviceType);
    await SecureStore.setItemAsync("syncmind.pairing.paired_at", defaultPairingState.pairedAt);
    await SecureStore.setItemAsync("syncmind.pairing.spine_url", defaultPairingState.spineUrl);
    await SecureStore.setItemAsync("syncmind.pairing.ca_fingerprint", "null");
    // NOTE: syncmind.pairing.last_seen_at is not set

    const restored = await restorePairingState();
    expect(restored).not.toBeNull();
    expect(restored!.lastSeenAt).toBeNull();
    expect(restored!.selfDeviceUuid).toBe(defaultPairingState.selfDeviceUuid);
  });

  it("clears lastSeenAt when clearing pairing state", async () => {
    await persistPairingState(defaultPairingState);
    await clearPairingState();

    const val = await SecureStore.getItemAsync("syncmind.pairing.last_seen_at");
    expect(val).toBeNull();
    expect(getRestoredPairingState()).toBeNull();
  });
});

// ── updateLastSeenAt throttle ──────────────────────────────────────────

describe("updateLastSeenAt", () => {
  it("updates in-memory lastSeenAt immediately", async () => {
    await persistPairingState(defaultPairingState);
    const before = getLastSeenAt();

    await updateLastSeenAt();
    const after = getLastSeenAt();

    expect(after).not.toBe(before);
    expect(after).toBeTruthy();
  });

  it("writes lastSeenAt to secure store when throttle allows", async () => {
    await persistPairingState(defaultPairingState);
    __resetLastSeenThrottleForTests();

    (SecureStore.setItemAsync as jest.Mock).mockClear();
    await updateLastSeenAt();

    const lastSeenCalls = (SecureStore.setItemAsync as jest.Mock).mock.calls.filter(
      (call: string[]) => call[0] === "syncmind.pairing.last_seen_at",
    );
    expect(lastSeenCalls.length).toBe(1);
  });

  it("skips secure-store write when within throttle window", async () => {
    await persistPairingState(defaultPairingState);
    await updateLastSeenAt(); // sets lastSeenPersistedAt = Date.now()

    (SecureStore.setItemAsync as jest.Mock).mockClear();
    await updateLastSeenAt(); // should be throttled

    const lastSeenCalls = (SecureStore.setItemAsync as jest.Mock).mock.calls.filter(
      (call: string[]) => call[0] === "syncmind.pairing.last_seen_at",
    );
    expect(lastSeenCalls.length).toBe(0);
  });

  it("getLastSeenAt returns null when no pairing state", () => {
    expect(getLastSeenAt()).toBeNull();
  });
});

// ── device_reset regression guard ──────────────────────────────────────

describe("device_reset", () => {
  it("still clears pairing state during a full reset", async () => {
    (global.fetch as jest.Mock).mockResolvedValueOnce(
      new Response(null, { status: 204 }),
    );

    await ensureIdentity();
    await persistPairingState(defaultPairingState);
    await device_reset();

    await expect(restorePairingState()).resolves.toBeNull();
  });
});

// ── authenticatedFetch ─────────────────────────────────────────────────

describe("authenticatedFetch", () => {
  beforeEach(async () => {
    await ensureIdentity();
  });

  it("reuses the current device JWT across successful authenticated requests", async () => {
    global.fetch = jest.fn(async () => new Response(null, { status: 204 })) as typeof fetch;
    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    await authenticatedFetch("https://spine.syncmind.local:8443/v1/devices/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    await authenticatedFetch("https://spine.syncmind.local:8443/v1/devices/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");

    expect(NativeDeviceIdentity.sign).toHaveBeenCalledTimes(1);
  });

  it("mints a fresh device JWT after a 401 response", async () => {
    global.fetch = jest
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      .mockResolvedValueOnce(new Response(null, { status: 204 })) as typeof fetch;
    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    await expect(
      authenticatedFetch("https://spine.syncmind.local:8443/v1/devices/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
    ).rejects.toThrow(UnpairedError);
    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    await authenticatedFetch("https://spine.syncmind.local:8443/v1/devices/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");

    expect(NativeDeviceIdentity.sign).toHaveBeenCalledTimes(2);
  });

  it("updates lastSeenAt on 2xx response", async () => {
    global.fetch = jest.fn(async () => new Response(JSON.stringify({ ok: true }), { status: 200 })) as typeof fetch;

    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    await authenticatedFetch("https://spine.syncmind.local:8443/v1/devices/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");

    const lastSeen = getLastSeenAt();
    expect(lastSeen).toBeTruthy();
  });

  it("uses standard device-issued JWT issuer and Spine audience claims", async () => {
    global.fetch = jest.fn(async () => new Response(JSON.stringify({ ok: true }), { status: 200 })) as typeof fetch;

    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    await authenticatedFetch("https://spine.syncmind.local:8443/v1/devices/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");

    const headers = (global.fetch as jest.Mock).mock.calls[0][1].headers as Record<string, string>;
    const token = headers.Authorization.replace(/^Bearer /, "");
    const payload = decodeJwtPayload(token);
    expect(payload.iss).toBe("syncmind-device");
    expect(payload.aud).toBe("syncmind-spine");
  });

  it("throws UnpairedError and clears state on 401", async () => {
    global.fetch = jest.fn(async () => new Response(JSON.stringify({ code: "AUTH_INVALID" }), { status: 401 })) as typeof fetch;

    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    await expect(
      authenticatedFetch("https://spine.syncmind.local:8443/v1/sync/bundles"),
    ).rejects.toThrow(UnpairedError);

    expect(useAppStore.getState().isPaired).toBe(false);
    await expect(restorePairingState()).resolves.toBeNull();
  });

  it("auto-unpairs on self-device 404 with DEVICE_REVOKED code", async () => {
    global.fetch = jest.fn(async () =>
      new Response(JSON.stringify({ code: "DEVICE_REVOKED", message: "device revoked" }), { status: 404 }),
    ) as typeof fetch;

    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    await expect(
      authenticatedFetch("https://spine.syncmind.local:8443/v1/sync/bundles"),
    ).rejects.toThrow(UnpairedError);

    expect(useAppStore.getState().isPaired).toBe(false);
  });

  it("auto-unpairs on self-device path 404 even without error code", async () => {
    global.fetch = jest.fn(async () => new Response("{}", { status: 404 })) as typeof fetch;

    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    await expect(
      authenticatedFetch(
        `https://spine.syncmind.local:8443/v1/devices/${defaultPairingState.selfDeviceUuid}/revoke`,
        { method: "POST" },
      ),
    ).rejects.toThrow(UnpairedError);

    expect(useAppStore.getState().isPaired).toBe(false);
  });

  it("does NOT clear pairing state on another device path 404 without device error code", async () => {
    global.fetch = jest.fn(async () => new Response("{}", { status: 404 })) as typeof fetch;

    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    const response = await authenticatedFetch(
      "https://spine.syncmind.local:8443/v1/devices/bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
    );

    expect(response.status).toBe(404);
    expect(useAppStore.getState().isPaired).toBe(true);
    expect(getRestoredPairingState()?.selfDeviceUuid).toBe(defaultPairingState.selfDeviceUuid);
  });

  it("does NOT clear pairing state on generic resource 404", async () => {
    global.fetch = jest.fn(async () => new Response(JSON.stringify({ code: "NOT_FOUND" }), { status: 404 })) as typeof fetch;

    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    const response = await authenticatedFetch(
      "https://spine.syncmind.local:8443/v1/sync/bundles/nonexistent",
    );

    expect(response.status).toBe(404);
    expect(useAppStore.getState().isPaired).toBe(true);
  });

  it("does NOT clear pairing state on network error", async () => {
    global.fetch = jest.fn(async () => { throw new TypeError("Network request failed"); }) as typeof fetch;

    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    await expect(
      authenticatedFetch("https://spine.syncmind.local:8443/v1/sync/bundles"),
    ).rejects.toThrow("Network request failed");

    expect(useAppStore.getState().isPaired).toBe(true);
  });
});

// ── self-device health check ───────────────────────────────────────────

describe("checkCurrentDevicePairing", () => {
  it("keeps pairing state when Spine still reports a paired peer", async () => {
    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);
    global.fetch = jest.fn(async () =>
      new Response(
        JSON.stringify({
          device_uuid: defaultPairingState.selfDeviceUuid,
          device_type: "mobile",
          paired_device_id: defaultPairingState.pairedPeerDeviceId,
          is_active: true,
          last_seen_at: null,
        }),
        { status: 200 },
      ),
    ) as typeof fetch;

    await checkCurrentDevicePairing({ allowJwtMint: true });

    expect(useAppStore.getState().isPaired).toBe(true);
    await expect(restorePairingState()).resolves.not.toBeNull();
  });

  it("clears pairing state when Spine reports the peer link is gone", async () => {
    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);
    global.fetch = jest.fn(async () =>
      new Response(
        JSON.stringify({
          device_uuid: defaultPairingState.selfDeviceUuid,
          device_type: "mobile",
          paired_device_id: null,
          is_active: true,
          last_seen_at: null,
        }),
        { status: 200 },
      ),
    ) as typeof fetch;

    await expect(checkCurrentDevicePairing({ allowJwtMint: true })).rejects.toThrow(UnpairedError);

    expect(useAppStore.getState().isPaired).toBe(false);
    await expect(restorePairingState()).resolves.toBeNull();
  });

  it("does not trigger interactive signing when a background health check has no cached JWT", async () => {
    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    await expect(checkCurrentDevicePairing()).rejects.toThrow(AuthPromptRequiredError);

    expect(NativeDeviceIdentity.sign).not.toHaveBeenCalled();
    expect(global.fetch).not.toHaveBeenCalled();
    expect(useAppStore.getState().isPaired).toBe(true);
  });
});

// ── unpair flow ────────────────────────────────────────────────────────

describe("unpair", () => {
  beforeEach(async () => {
    await ensureIdentity();
  });

  it("clears pairing state, outbox, and sets unpaired without clearing identity", async () => {
    (global.fetch as jest.Mock).mockResolvedValueOnce(
      new Response(null, { status: 204 }),
    );

    await persistPairingState(defaultPairingState);
    await enqueueOutboxItem("cap-1", new Uint8Array(32).fill(0xff));
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    const result = await unpair();

    expect(result.revokeWarning).toBeNull();
    expect(useAppStore.getState().isPaired).toBe(false);
    await expect(restorePairingState()).resolves.toBeNull();
    await expect(getOutboxItems()).resolves.toEqual([]);
  });

  it("preserves the mobile device UUID for re-pairing after unpair", async () => {
    (global.fetch as jest.Mock).mockResolvedValueOnce(
      new Response(null, { status: 204 }),
    );

    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    await unpair();

    await expect(ensureMobileDeviceUuid()).resolves.toBe(
      defaultPairingState.selfDeviceUuid,
    );
  });

  it("returns revokeWarning on network error but still completes local cleanup", async () => {
    (global.fetch as jest.Mock).mockRejectedValueOnce(new TypeError("Network request failed"));

    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    const result = await unpair();

    expect(result.revokeWarning).toBe("network_error");
    expect(useAppStore.getState().isPaired).toBe(false);
    await expect(restorePairingState()).resolves.toBeNull();
  });

  it("treats UnpairedError from revoke as silent success", async () => {
    (global.fetch as jest.Mock).mockResolvedValueOnce(
      new Response(JSON.stringify({ code: "AUTH_INVALID" }), { status: 401 }),
    );

    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    const result = await unpair();

    expect(result.revokeWarning).toBeNull();
    expect(useAppStore.getState().isPaired).toBe(false);
  });

  it("aborts in-flight authenticated Spine requests for the current pairing during unpair", async () => {
    const abortError = new DOMException("Aborted", "AbortError");
    let inFlightSignal: AbortSignal | undefined;
    let resolveFetchStarted: (() => void) | undefined;
    const fetchStarted = new Promise<void>((resolve) => {
      resolveFetchStarted = resolve;
    });

    global.fetch = jest
      .fn()
      .mockImplementationOnce((_url: string, init?: RequestInit) => {
        inFlightSignal = init?.signal ?? undefined;
        resolveFetchStarted?.();
        return new Promise<Response>((_resolve, reject) => {
          inFlightSignal?.addEventListener("abort", () => reject(abortError));
        });
      })
      .mockResolvedValueOnce(new Response(null, { status: 204 })) as typeof fetch;

    await persistPairingState(defaultPairingState);
    await enqueueOutboxItem("cap-1", new Uint8Array(32).fill(0xff));
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    const inFlight = authenticatedFetch(
      "https://spine.syncmind.local:8443/v1/sync/uploads/cap-1",
      { method: "POST", body: "payload" },
    );

    await fetchStarted;
    expect(inFlightSignal).toBeDefined();
    expect(inFlightSignal!.aborted).toBe(false);

    const result = await unpair();

    expect(result.revokeWarning).toBeNull();
    expect(inFlightSignal!.aborted).toBe(true);
    await expect(inFlight).rejects.toThrow("Aborted");
    expect(useAppStore.getState().isPaired).toBe(false);
    await expect(restorePairingState()).resolves.toBeNull();
    await expect(getOutboxItems()).resolves.toEqual([]);
  });
});
