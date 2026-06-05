import * as Crypto from "expo-crypto";
import { gcm } from "@noble/ciphers/aes";
import { sha256 } from "@noble/hashes/sha2.js";
import type { PersistedPairingState } from "../spine/session";

export interface CaptureTextPayload {
  v: 1;
  kind: "capture-text";
  id: string;
  text: string;
  source: "typed";
  client_ts: string;
  client_device_fingerprint: string;
}

export type CaptureTextPayloadInput = Omit<
  CaptureTextPayload,
  "v" | "kind" | "source"
>;

export interface CaptureAudioPayload {
  v: 1;
  kind: "capture-audio";
  id: string;
  audio_base64: string;
  audio_mime: "audio/mp4";
  duration_ms: number;
  client_ts: string;
  client_device_fingerprint: string;
}

export type CaptureAudioPayloadInput = Omit<
  CaptureAudioPayload,
  "v" | "kind" | "audio_mime"
>;

export interface BundleEnvelope {
  schema_version: number;
  kind: string;
  filename: string;
  content_utf8: string;
  captured_at: string;
  sha256: string;
}

interface EncryptBundleParams {
  envelope: BundleEnvelope;
  syncKey: Uint8Array;
  peerFingerprintAAD: Uint8Array;
}

export interface EncryptedBundle {
  blob: Uint8Array;
  payloadHash: string;
}

const SECURE_SERIALIZE_BYPASS = new WeakSet<object>();
const GUARDED_TO_JSON = "Capture bundle payloads must use secureSerialize()";

/**
 * The only allowed path from a capture payload object to UTF-8 JSON bytes.
 * All bundle encryption paths MUST use this instead of direct JSON.stringify.
 */
export function secureSerialize<T>(payload: T): Uint8Array {
  const encoder = new TextEncoder();
  if (payload && typeof payload === "object") {
    SECURE_SERIALIZE_BYPASS.add(payload);
    try {
      return encoder.encode(JSON.stringify(payload));
    } finally {
      SECURE_SERIALIZE_BYPASS.delete(payload);
    }
  }
  return encoder.encode(JSON.stringify(payload));
}

export function createCaptureTextPayload(
  payload: CaptureTextPayloadInput,
): CaptureTextPayload {
  return withSecureSerializeGuard({
    v: 1,
    kind: "capture-text",
    ...payload,
    source: "typed",
  });
}

export function createCaptureAudioPayload(
  payload: CaptureAudioPayloadInput,
): CaptureAudioPayload {
  return withSecureSerializeGuard({
    v: 1,
    kind: "capture-audio",
    ...payload,
    audio_mime: "audio/mp4",
  });
}

function withSecureSerializeGuard<T extends object>(payload: T): T {
  Object.defineProperty(payload, "toJSON", {
    enumerable: false,
    configurable: false,
    value() {
      if (!SECURE_SERIALIZE_BYPASS.has(payload)) {
        throw new Error(GUARDED_TO_JSON);
      }
      return { ...payload };
    },
  });
  return payload;
}

/**
 * Decode the peer fingerprint `sha256:<hex>` to raw 32-byte AAD.
 */
export function peerFingerprintToAAD(fingerprint: string): Uint8Array {
  if (!fingerprint.startsWith("sha256:")) {
    throw new Error("Invalid peer fingerprint: must start with sha256:");
  }

  const hex = fingerprint.slice(7);
  if (hex.length !== 64) {
    throw new Error("Invalid peer fingerprint: hex part must be 64 chars");
  }

  const bytes = new Uint8Array(32);
  for (let i = 0; i < 32; i++) {
    const byte = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
    if (Number.isNaN(byte)) {
      throw new Error("Invalid peer fingerprint: non-hex characters");
    }
    bytes[i] = byte;
  }
  return bytes;
}

/**
 * Build the inner capture text payload and outer BundleEnvelope.
 */
export function buildCaptureTextEnvelope(
  payload: CaptureTextPayloadInput,
): BundleEnvelope {
  const capturePayload = createCaptureTextPayload(payload);
  const contentUtf8 = new TextDecoder().decode(secureSerialize(capturePayload));
  const sha256 = computeLowerHexSha256(new TextEncoder().encode(contentUtf8));

  return {
    schema_version: 1,
    kind: "capture-text",
    filename: `capture-${payload.id}.json`,
    content_utf8: contentUtf8,
    captured_at: new Date().toISOString(),
    sha256,
  };
}

/**
 * Build the inner capture audio payload and outer BundleEnvelope.
 */
export function buildCaptureAudioEnvelope(
  payload: CaptureAudioPayloadInput,
): BundleEnvelope {
  const capturePayload = createCaptureAudioPayload(payload);
  const contentUtf8 = new TextDecoder().decode(secureSerialize(capturePayload));
  const sha256 = computeLowerHexSha256(new TextEncoder().encode(contentUtf8));

  return {
    schema_version: 1,
    kind: "capture-audio",
    filename: `capture-${payload.id}.json`,
    content_utf8: contentUtf8,
    captured_at: new Date().toISOString(),
    sha256,
  };
}

/**
 * Encrypt a bundle envelope using AES-256-GCM with the paired sync key.
 * Returns nonce(12) | ciphertext_and_tag blob and SHA-256 of the encrypted blob.
 */
export async function encryptBundle(params: EncryptBundleParams): Promise<EncryptedBundle> {
  const { envelope, syncKey, peerFingerprintAAD } = params;

  if (syncKey.length !== 32) {
    throw new Error("syncKey must be exactly 32 bytes");
  }

  const plaintext = secureSerialize(envelope);
  const nonce = Crypto.getRandomBytes(12);
  const cipher = gcm(syncKey, nonce, peerFingerprintAAD);
  const ciphertext = cipher.encrypt(plaintext);

  // Concatenate nonce | ciphertext_and_tag
  const blob = new Uint8Array(nonce.length + ciphertext.length);
  blob.set(nonce, 0);
  blob.set(ciphertext, nonce.length);

  const payloadHash = computeLowerHexSha256(blob);

  return { blob, payloadHash };
}

/**
 * Encrypt a capture text payload into a bundle-ready encrypted blob.
 */
export async function encryptCaptureText(
  textPayload: CaptureTextPayload,
  state: PersistedPairingState,
): Promise<EncryptedBundle & { id: string }> {
  const envelope = buildCaptureTextEnvelope(createCaptureTextPayload(textPayload));
  const aad = peerFingerprintToAAD(state.pairedPeerFingerprint);
  const result = await encryptBundle({
    envelope,
    syncKey: state.syncKey,
    peerFingerprintAAD: aad,
  });
  return { ...result, id: textPayload.id };
}

/**
 * Encrypt a capture audio payload into a bundle-ready encrypted blob.
 */
export async function encryptCaptureAudio(
  audioPayload: CaptureAudioPayload,
  state: PersistedPairingState,
): Promise<EncryptedBundle & { id: string }> {
  const envelope = buildCaptureAudioEnvelope(createCaptureAudioPayload(audioPayload));
  const aad = peerFingerprintToAAD(state.pairedPeerFingerprint);
  const result = await encryptBundle({
    envelope,
    syncKey: state.syncKey,
    peerFingerprintAAD: aad,
  });
  return { ...result, id: audioPayload.id };
}

function computeLowerHexSha256(data: Uint8Array): string {
  const digest: Uint8Array = sha256.create().update(data).digest();
  return Array.from(digest)
    .map((b: number) => b.toString(16).padStart(2, "0"))
    .join("");
}
