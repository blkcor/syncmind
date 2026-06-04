import * as Crypto from "expo-crypto";
import { sign } from "../crypto/identity";
import { bytesToBase64UrlNoPad } from "../pairing/encoding";
import { useAppStore } from "../store";
import {
  clearPairingState,
  getRestoredPairingState,
  type PersistedPairingState,
  updateLastSeenAt,
} from "./session";

export class UnpairedError extends Error {
  constructor(
    message: string,
    public readonly statusCode: number,
  ) {
    super(message);
    this.name = "UnpairedError";
  }
}

export class AuthPromptRequiredError extends Error {
  constructor() {
    super("Interactive device authentication is required");
    this.name = "AuthPromptRequiredError";
  }
}

async function createDeviceJWT(state: PersistedPairingState): Promise<string> {
  const now = Math.floor(Date.now() / 1000);
  const header = { alg: "EdDSA", typ: "JWT" };
  const payload = {
    sub: state.selfDeviceUuid,
    iss: "syncmind-device",
    aud: "syncmind-spine",
    iat: now,
    exp: now + 86_400,
    jti: Crypto.randomUUID(),
  };

  const encoder = new TextEncoder();
  const headerB64 = bytesToBase64UrlNoPad(encoder.encode(JSON.stringify(header)));
  const payloadB64 = bytesToBase64UrlNoPad(encoder.encode(JSON.stringify(payload)));
  const signingInput = `${headerB64}.${payloadB64}`;

  const signatureBytes = await sign(encoder.encode(signingInput));
  const signatureB64 = bytesToBase64UrlNoPad(signatureBytes);

  return `${signingInput}.${signatureB64}`;
}

const inFlightByPairing = new Map<string, Set<AbortController>>();

function isSelfDevicePath(url: string, selfDeviceUuid: string): boolean {
  let pathname: string;
  try {
    pathname = new URL(url).pathname;
  } catch {
    pathname = url;
  }

  return (
    pathname === `/v1/devices/${selfDeviceUuid}` ||
    pathname === `/v1/devices/${selfDeviceUuid}/revoke`
  );
}

function trackInFlightRequest(
  selfDeviceUuid: string,
  controller: AbortController,
): () => void {
  const controllers = inFlightByPairing.get(selfDeviceUuid) ?? new Set<AbortController>();
  controllers.add(controller);
  inFlightByPairing.set(selfDeviceUuid, controllers);

  return () => {
    controllers.delete(controller);
    if (controllers.size === 0) {
      inFlightByPairing.delete(selfDeviceUuid);
    }
  };
}

export function abortInFlightSpineWorkForPairing(selfDeviceUuid: string): void {
  const controllers = inFlightByPairing.get(selfDeviceUuid);
  if (!controllers) {
    return;
  }

  for (const controller of controllers) {
    controller.abort();
  }
  controllers.clear();
  inFlightByPairing.delete(selfDeviceUuid);
}

function createMergedAbortController(signal?: AbortSignal | null): {
  controller: AbortController;
  cleanup: () => void;
} {
  const controller = new AbortController();
  if (!signal) {
    return { controller, cleanup: () => {} };
  }

  if (signal.aborted) {
    controller.abort();
    return { controller, cleanup: () => {} };
  }

  const abort = () => controller.abort();
  signal.addEventListener("abort", abort, { once: true });
  return {
    controller,
    cleanup: () => signal.removeEventListener("abort", abort),
  };
}

interface SpineErrorBody {
  code?: string;
  message?: string;
}

interface DeviceStatusResponse {
  device_uuid: string;
  device_type: string;
  paired_device_id: string | null;
  is_active: boolean;
  last_seen_at: string | null;
}

interface CachedDeviceJWT {
  selfDeviceUuid: string;
  token: string;
  expiresAtSec: number;
}

const JWT_TTL_SEC = 86_400;
const JWT_REFRESH_WINDOW_SEC = 300;
let cachedDeviceJWT: CachedDeviceJWT | null = null;

interface AuthenticatedFetchOptions extends RequestInit {
  allowJwtMint?: boolean;
}

export async function authenticatedFetch(
  url: string,
  options: AuthenticatedFetchOptions = {},
): Promise<Response> {
  const state = getRestoredPairingState();
  if (!state) {
    throw new Error("Cannot create JWT: no pairing state");
  }

  const { allowJwtMint = true, ...fetchOptions } = options;
  const jwt = await currentDeviceJWT(state, allowJwtMint);
  const { controller, cleanup: cleanupMergedSignal } = createMergedAbortController(
    fetchOptions.signal,
  );
  const cleanupInFlight = trackInFlightRequest(state.selfDeviceUuid, controller);

  const authHeaders = {
    ...(fetchOptions.headers as Record<string, string> | undefined),
    Authorization: `Bearer ${jwt}`,
  };

  try {
    const response = await fetch(url, {
      ...fetchOptions,
      headers: authHeaders,
      signal: controller.signal,
    });

    if (response.ok) {
      await updateLastSeenAt();
      return response;
    }

    if (response.status === 401) {
      clearCachedDeviceJWT();
      await clearPairingState();
      useAppStore.getState().setUnpaired();
      throw new UnpairedError("Device authentication failed", 401);
    }

    if (response.status === 404) {
      let body: SpineErrorBody | null = null;
      try {
        body = await response.clone().json();
      } catch {
        // body not parseable, fall through to path-based check
      }

      if (
        body?.code === "DEVICE_REVOKED" ||
        body?.code === "DEVICE_NOT_FOUND" ||
        isSelfDevicePath(url, state.selfDeviceUuid)
      ) {
        clearCachedDeviceJWT();
        await clearPairingState();
        useAppStore.getState().setUnpaired();
        throw new UnpairedError(
          body?.message ?? "Device not found or revoked",
          404,
        );
      }
    }

    return response;
  } finally {
    cleanupInFlight();
    cleanupMergedSignal();
  }
}

async function currentDeviceJWT(
  state: PersistedPairingState,
  allowMint: boolean,
): Promise<string> {
  const now = Math.floor(Date.now() / 1000);
  if (
    cachedDeviceJWT &&
    cachedDeviceJWT.selfDeviceUuid === state.selfDeviceUuid &&
    cachedDeviceJWT.expiresAtSec - JWT_REFRESH_WINDOW_SEC > now
  ) {
    return cachedDeviceJWT.token;
  }

  if (!allowMint) {
    throw new AuthPromptRequiredError();
  }

  const token = await createDeviceJWT(state);
  cachedDeviceJWT = {
    selfDeviceUuid: state.selfDeviceUuid,
    token,
    expiresAtSec: now + JWT_TTL_SEC,
  };
  return token;
}

function clearCachedDeviceJWT(): void {
  cachedDeviceJWT = null;
}

export function __resetDeviceJWTCacheForTests(): void {
  clearCachedDeviceJWT();
}

export async function revokeCurrentDevice(): Promise<void> {
  const state = getRestoredPairingState();
  if (!state) {
    return;
  }

  let response: Response;
  try {
    response = await authenticatedFetch(
      `${state.spineUrl}/v1/devices/${state.selfDeviceUuid}/revoke`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ device_uuid: state.selfDeviceUuid }),
      },
    );
  } catch (err) {
    if (err instanceof UnpairedError) {
      return;
    }
    throw err;
  }

  if (!response.ok) {
    throw new Error(`Failed to revoke current device: ${response.status}`);
  }
}

export async function checkCurrentDevicePairing(
  options: { allowJwtMint?: boolean } = {},
): Promise<DeviceStatusResponse> {
  const state = getRestoredPairingState();
  if (!state) {
    throw new Error("Cannot check pairing: no pairing state");
  }

  const response = await authenticatedFetch(
    `${state.spineUrl}/v1/devices/${state.selfDeviceUuid}`,
    { allowJwtMint: options.allowJwtMint ?? false },
  );
  if (!response.ok) {
    throw new Error(`Failed to check current device: ${response.status}`);
  }

  const body = (await response.json()) as DeviceStatusResponse;
  if (!body.is_active || body.paired_device_id === null) {
    clearCachedDeviceJWT();
    await clearPairingState();
    useAppStore.getState().setUnpaired();
    throw new UnpairedError("Paired device link no longer exists", 404);
  }

  return body;
}
