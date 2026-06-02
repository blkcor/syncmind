import { ensureIdentity, getDevicePubkey } from "../crypto/identity";
import {
  persistPairingState,
  restorePairingState,
  type PersistedPairingState,
} from "../spine/session";
import { useAppStore } from "../store";
import { ensureMobileDeviceUuid } from "./device";
import {
  completePairing,
  decodePairingPubkey,
  deriveSyncKey,
  ed25519PublicKeyToX25519,
  PairingFingerprintConflictError,
} from "./handshake";
import type { PairingPayload } from "./payload";
import {
  PairingPayloadError,
  validatePairingPayload,
  validationErrorMessage,
} from "./payload";
import { verifyPresentedCertificateFingerprint } from "./tls-check";

export * from "./device";
export * from "./handshake";
export * from "./payload";
export * from "./tls-check";

export async function startPairingFlow(
  payload: PairingPayload,
): Promise<PersistedPairingState> {
  const validation = validatePairingPayload(payload, { allowHttp: Boolean(__DEV__) });
  if (validation) {
    throw new PairingPayloadError(validation);
  }

  await verifyPresentedCertificateFingerprint(payload.ca_fingerprint);

  const previousState = await restorePairingState();
  await ensureIdentity();
  const selfDeviceUuid = await ensureMobileDeviceUuid();
  const identityPubkey = getDevicePubkey();
  const { completion, selfDeviceUuid: completedSelfDeviceUuid } = await completePairingWithDeviceUuidRecovery(
    payload,
    selfDeviceUuid,
    identityPubkey,
  );
  const peerEd25519Pubkey = decodePairingPubkey(
    completion.initiator_pubkey ?? payload.device_a_pubkey,
  );
  const peerX25519Pubkey = ed25519PublicKeyToX25519(peerEd25519Pubkey);
  const syncKey = await deriveSyncKey(peerX25519Pubkey, payload.session_id);

  const state: PersistedPairingState = {
    selfDeviceUuid: completedSelfDeviceUuid,
    syncKey,
    pairedPeerFingerprint: payload.device_a_fingerprint,
    pairedPeerDeviceId: completion.initiator_id,
    pairedPeerDeviceType: "desktop",
    pairedAt: new Date().toISOString(),
    spineUrl: payload.spine_url,
    caFingerprint: payload.ca_fingerprint,
    lastSeenAt: null,
  };

  await persistPairingState(state);
  useAppStore
    .getState()
    .setPaired(payload.device_a_fingerprint, !previousState?.pairedPeerFingerprint);

  return state;
}

async function completePairingWithDeviceUuidRecovery(
  payload: PairingPayload,
  selfDeviceUuid: string,
  identityPubkey: Uint8Array | string,
) {
  try {
    return {
      completion: await completePairing(payload, selfDeviceUuid, identityPubkey),
      selfDeviceUuid,
    };
  } catch (error) {
    if (
      error instanceof PairingFingerprintConflictError &&
      error.existingDeviceUuid
    ) {
      const completion = await completePairing(
        payload,
        error.existingDeviceUuid,
        identityPubkey,
      );
      return { completion, selfDeviceUuid: error.existingDeviceUuid };
    }
    throw error;
  }
}

export function pairingFlowErrorMessage(error: unknown, payload?: PairingPayload): string {
  if (error instanceof PairingPayloadError) {
    return validationErrorMessage(error);
  }
  if (error instanceof Error) {
    return error.message;
  }
  if (payload) {
    return `Cannot reach ${payload.spine_url} - check your network connection`;
  }
  return "Pairing failed - please try again";
}
