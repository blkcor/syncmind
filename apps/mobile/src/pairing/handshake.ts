import { hkdf } from "@noble/hashes/hkdf.js";
import { sha256 } from "@noble/hashes/sha2.js";
import { utf8ToBytes } from "@noble/hashes/utils.js";
import { ed25519 } from "@noble/curves/ed25519.js";
import { derive_x25519 } from "../crypto/identity";
import type { PairingPayload } from "./payload";
import {
  base64UrlNoPadToBytes,
  bytesToBase64UrlNoPad,
} from "./encoding";

const DEVICE_TYPE = "mobile";
const HKDF_INFO = "syncmind-v1";
const FIELD_PRIME = (1n << 255n) - 19n;

export interface PairingCompleteResponse {
  status: "completed";
  session_id: string;
  initiator_id: string;
  responder_id: string;
  initiator_pubkey?: string;
}

export async function completePairing(
  payload: PairingPayload,
  selfDeviceUuid: string,
  identityPubkey: Uint8Array | string,
): Promise<PairingCompleteResponse> {
  const responder_pubkey =
    typeof identityPubkey === "string"
      ? identityPubkey
      : bytesToBase64UrlNoPad(identityPubkey);

  const response = await fetch(`${payload.spine_url}/v1/pairing/complete`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      session_id: payload.session_id,
      device_uuid: selfDeviceUuid,
      responder_pubkey,
      device_type: DEVICE_TYPE,
    }),
  });

  if (!response.ok) {
    throw await pairingHttpError(response, payload.spine_url);
  }

  const body = (await response.json()) as Partial<PairingCompleteResponse>;
  if (
    body.status !== "completed" ||
    !body.session_id ||
    !body.initiator_id ||
    !body.responder_id
  ) {
    throw new Error("Invalid pairing completion response.");
  }
  if (body.initiator_pubkey && body.initiator_pubkey !== payload.device_a_pubkey) {
    throw new Error("Pairing initiator public key mismatch.");
  }

  return {
    status: body.status,
    session_id: body.session_id,
    initiator_id: body.initiator_id,
    responder_id: body.responder_id,
    initiator_pubkey: body.initiator_pubkey,
  };
}

export async function deriveSyncKey(
  peerX25519Pubkey: Uint8Array,
  sessionId: string,
): Promise<Uint8Array> {
  const sharedSecret = await derive_x25519(peerX25519Pubkey);
  return hkdf(
    sha256,
    sharedSecret,
    utf8ToBytes(sessionId),
    utf8ToBytes(HKDF_INFO),
    32,
  );
}

export function decodePairingPubkey(value: string): Uint8Array {
  const bytes = base64UrlNoPadToBytes(value);
  if (bytes.length !== 32) {
    throw new Error("Pairing public key must be 32 bytes.");
  }
  return bytes;
}

export function ed25519PublicKeyToX25519(ed25519PublicKey: Uint8Array): Uint8Array {
  const point = ed25519.Point.fromBytes(ed25519PublicKey).toAffine();
  const u = mod((1n + point.y) * invert(1n - point.y));
  return littleEndianBytes(u);
}

async function pairingHttpError(
  response: Response,
  spineUrl: string,
): Promise<Error> {
  let code: string | undefined;
  try {
    const body = (await response.json()) as { code?: string; error?: string };
    code = body.code ?? body.error;
  } catch {
    code = undefined;
  }

  if (response.status === 410 || code === "PAIRING_EXPIRED") {
    return new Error("QR code expired - please generate a new one from the desktop Devices panel");
  }
  if (response.status === 409 && code === "PAIRING_ALREADY_COMPLETED") {
    return new Error(
      "This QR code has already been used - if someone else paired your desktop, check your Devices panel",
    );
  }
  if (response.status === 409 && code === "UUID_CONFLICT") {
    return new Error(
      "This mobile identity is already registered differently - reset device identity before pairing again",
    );
  }
  return new Error(`Cannot reach ${spineUrl} - check your network connection`);
}

function mod(value: bigint): bigint {
  const result = value % FIELD_PRIME;
  return result >= 0n ? result : result + FIELD_PRIME;
}

function invert(value: bigint): bigint {
  return pow(mod(value), FIELD_PRIME - 2n);
}

function pow(base: bigint, exponent: bigint): bigint {
  let result = 1n;
  let current = base;
  let remaining = exponent;
  while (remaining > 0n) {
    if (remaining & 1n) {
      result = mod(result * current);
    }
    current = mod(current * current);
    remaining >>= 1n;
  }
  return result;
}

function littleEndianBytes(value: bigint): Uint8Array {
  const bytes = new Uint8Array(32);
  let remaining = value;
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = Number(remaining & 0xffn);
    remaining >>= 8n;
  }
  return bytes;
}
