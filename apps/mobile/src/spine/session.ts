import * as SecureStore from "expo-secure-store";
import { base64ToBytes, bytesToBase64 } from "../pairing/encoding";

export interface SpineSession {
  baseUrl: string;
  accessToken: string;
}

export interface PersistedPairingState {
  selfDeviceUuid: string;
  syncKey: Uint8Array;
  pairedPeerFingerprint: string;
  pairedPeerDeviceId: string;
  pairedPeerDeviceType: "desktop" | "mobile";
  pairedAt: string;
  spineUrl: string;
  caFingerprint: string | null;
  lastSeenAt: string | null;
}

const PAIRING_KEYS = {
  selfDeviceUuid: "syncmind.pairing.self_device_uuid",
  syncKey: "syncmind.pairing.sync_key",
  pairedPeerFingerprint: "syncmind.pairing.paired_peer_fingerprint",
  pairedPeerDeviceId: "syncmind.pairing.paired_peer_device_id",
  pairedPeerDeviceType: "syncmind.pairing.paired_peer_device_type",
  pairedAt: "syncmind.pairing.paired_at",
  spineUrl: "syncmind.pairing.spine_url",
  caFingerprint: "syncmind.pairing.ca_fingerprint",
  lastSeenAt: "syncmind.pairing.last_seen_at",
} as const;

let currentSession: SpineSession | null = null;
let currentPairingState: PersistedPairingState | null = null;

export async function getCurrentSpineSession(): Promise<SpineSession | null> {
  return currentSession;
}

export async function setCurrentSpineSession(session: SpineSession): Promise<void> {
  currentSession = { ...session };
}

export async function clearCurrentSpineSession(): Promise<void> {
  currentSession = null;
}

export async function persistPairingState(
  state: PersistedPairingState,
): Promise<void> {
  await SecureStore.setItemAsync(PAIRING_KEYS.selfDeviceUuid, state.selfDeviceUuid);
  await SecureStore.setItemAsync(PAIRING_KEYS.syncKey, bytesToBase64(state.syncKey));
  await SecureStore.setItemAsync(
    PAIRING_KEYS.pairedPeerFingerprint,
    state.pairedPeerFingerprint,
  );
  await SecureStore.setItemAsync(
    PAIRING_KEYS.pairedPeerDeviceId,
    state.pairedPeerDeviceId,
  );
  await SecureStore.setItemAsync(
    PAIRING_KEYS.pairedPeerDeviceType,
    state.pairedPeerDeviceType,
  );
  await SecureStore.setItemAsync(PAIRING_KEYS.pairedAt, state.pairedAt);
  await SecureStore.setItemAsync(PAIRING_KEYS.spineUrl, state.spineUrl);
  await SecureStore.setItemAsync(
    PAIRING_KEYS.caFingerprint,
    state.caFingerprint ?? "null",
  );
  await SecureStore.setItemAsync(
    PAIRING_KEYS.lastSeenAt,
    state.lastSeenAt ?? "null",
  );
  currentPairingState = clonePairingState(state);
}

export async function restorePairingState(): Promise<PersistedPairingState | null> {
  const [
    selfDeviceUuid,
    syncKeyBase64,
    pairedPeerFingerprint,
    pairedPeerDeviceId,
    pairedPeerDeviceType,
    pairedAt,
    spineUrl,
    caFingerprint,
    lastSeenAt,
  ] = await Promise.all([
    SecureStore.getItemAsync(PAIRING_KEYS.selfDeviceUuid),
    SecureStore.getItemAsync(PAIRING_KEYS.syncKey),
    SecureStore.getItemAsync(PAIRING_KEYS.pairedPeerFingerprint),
    SecureStore.getItemAsync(PAIRING_KEYS.pairedPeerDeviceId),
    SecureStore.getItemAsync(PAIRING_KEYS.pairedPeerDeviceType),
    SecureStore.getItemAsync(PAIRING_KEYS.pairedAt),
    SecureStore.getItemAsync(PAIRING_KEYS.spineUrl),
    SecureStore.getItemAsync(PAIRING_KEYS.caFingerprint),
    SecureStore.getItemAsync(PAIRING_KEYS.lastSeenAt),
  ]);

  if (
    !selfDeviceUuid ||
    !syncKeyBase64 ||
    !pairedPeerFingerprint ||
    !pairedPeerDeviceId ||
    !pairedPeerDeviceType ||
    !pairedAt ||
    !spineUrl ||
    (pairedPeerDeviceType !== "desktop" && pairedPeerDeviceType !== "mobile")
  ) {
    currentPairingState = null;
    return null;
  }

  const state: PersistedPairingState = {
    selfDeviceUuid,
    syncKey: base64ToBytes(syncKeyBase64),
    pairedPeerFingerprint,
    pairedPeerDeviceId,
    pairedPeerDeviceType,
    pairedAt,
    spineUrl,
    caFingerprint: caFingerprint === "null" ? null : caFingerprint,
    lastSeenAt: lastSeenAt === "null" || lastSeenAt === null ? null : lastSeenAt,
  };
  currentPairingState = clonePairingState(state);
  return state;
}

export async function clearPairingState(): Promise<void> {
  await Promise.all(
    Object.values(PAIRING_KEYS).map((key) => SecureStore.deleteItemAsync(key)),
  );
  currentPairingState = null;
}

export function getRestoredPairingState(): PersistedPairingState | null {
  return currentPairingState ? clonePairingState(currentPairingState) : null;
}

let lastSeenPersistedAt = 0;
const LAST_SEEN_THROTTLE_MS = 30_000;

export function __clearPairingStateForTests(): void {
  currentPairingState = null;
}

export function __resetLastSeenThrottleForTests(): void {
  lastSeenPersistedAt = 0;
}

export async function updateLastSeenAt(): Promise<void> {
  const now = new Date().toISOString();
  if (currentPairingState) {
    currentPairingState.lastSeenAt = now;
  }
  if (Date.now() - lastSeenPersistedAt < LAST_SEEN_THROTTLE_MS) {
    return;
  }
  lastSeenPersistedAt = Date.now();
  await SecureStore.setItemAsync(PAIRING_KEYS.lastSeenAt, now);
}

export function getLastSeenAt(): string | null {
  return currentPairingState?.lastSeenAt ?? null;
}

function clonePairingState(state: PersistedPairingState): PersistedPairingState {
  return {
    ...state,
    syncKey: new Uint8Array(state.syncKey),
  };
}
