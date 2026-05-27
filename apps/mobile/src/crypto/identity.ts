import { ed25519, x25519 } from "@noble/curves/ed25519.js";
import { sha256, sha512 } from "@noble/hashes/sha2.js";
import { bytesToHex, hexToBytes } from "@noble/hashes/utils.js";
import * as SecureStore from "expo-secure-store";
import { clearOutbox } from "../outbox/service";
import { revokeCurrentDevice } from "../spine/client";
import { useAppStore } from "../store";

const STORAGE_KEY = "device_identity";

interface SerializedIdentity {
  privateKeyHex: string;
  publicKeyHex: string;
  fingerprint: string;
}

// Module-scoped in-memory cache — NOT exported.
// These values are never passed into React component trees, stores,
// serialization paths, or any code outside this module's exported functions.
let _privateKey: Uint8Array | null = null;
let _publicKey: Uint8Array | null = null;
let _fingerprint: string | null = null;
let _requireAuthentication = false;

function hexToUint8Array(hex: string): Uint8Array {
  return hexToBytes(hex);
}

function uint8ArrayToHex(bytes: Uint8Array): string {
  return bytesToHex(bytes);
}

/** Compute sha256:<hex> fingerprint of a public key. */
function computeFingerprint(publicKey: Uint8Array): string {
  const hash = sha256.create().update(publicKey).digest();
  return `sha256:${bytesToHex(hash)}`;
}

/**
 * Convert an Ed25519 private key seed to an X25519 private scalar
 * per RFC 7748 section 5.
 *
 * SHA-512 the seed and clamp the first 32 bytes:
 * - clear bit 0 of byte 0
 * - clear bit 7 of byte 31
 * - set bit 6 of byte 31
 */
function ed25519SeedToX25519Priv(seed: Uint8Array): Uint8Array {
  const hash = sha512.create().update(seed).digest();
  const scalar = hash.slice(0, 32);
  scalar[0] &= 248;
  scalar[31] &= 127;
  scalar[31] |= 64;
  return scalar;
}

/**
 * Ensure a device identity exists, generating one if necessary.
 * Returns the device fingerprint string (`sha256:<hex>`).
 */
export async function ensureIdentity(): Promise<string> {
  if (_privateKey && _publicKey && _fingerprint) {
    return _fingerprint;
  }

  try {
    const stored = await SecureStore.getItemAsync(STORAGE_KEY);
    if (stored) {
      const data: SerializedIdentity = JSON.parse(stored);
      _privateKey = hexToUint8Array(data.privateKeyHex);
      _publicKey = hexToUint8Array(data.publicKeyHex);
      _fingerprint = data.fingerprint;
    }
  } catch {
    // Corrupted storage — fall through to generate a fresh identity
  }

  if (!_privateKey || !_publicKey) {
    const privateKey = ed25519.utils.randomSecretKey();
    const publicKey = ed25519.getPublicKey(privateKey);
    const fingerprint = computeFingerprint(publicKey);

    const data: SerializedIdentity = {
      privateKeyHex: uint8ArrayToHex(privateKey),
      publicKeyHex: uint8ArrayToHex(publicKey),
      fingerprint,
    };

    await SecureStore.setItemAsync(STORAGE_KEY, JSON.stringify(data), {
      requireAuthentication: false,
    });

    _privateKey = privateKey;
    _publicKey = publicKey;
    _fingerprint = fingerprint;
    _requireAuthentication = false;
  }

  return _fingerprint!;
}

/** Return the cached device fingerprint. */
export function getDeviceFingerprint(): string {
  if (!_fingerprint) {
    throw new Error("Device identity not initialized. Call ensureIdentity() first.");
  }
  return _fingerprint;
}

/** Return the cached public key bytes. */
export function getDevicePubkey(): Uint8Array {
  if (!_publicKey) {
    throw new Error("Device identity not initialized. Call ensureIdentity() first.");
  }
  return new Uint8Array(_publicKey);
}

export function isAuthenticationRequired(): boolean {
  return _requireAuthentication;
}

/**
 * Sign a message with the device's Ed25519 private key.
 * Returns the raw 64-byte signature.
 */
export function sign(message: Uint8Array): Uint8Array {
  if (!_privateKey) {
    throw new Error("Device identity not initialized. Call ensureIdentity() first.");
  }
  return ed25519.sign(message, _privateKey);
}

/**
 * Derive an X25519 shared secret with a peer's X25519 public key.
 * Converts the internal Ed25519 private key to X25519 internally.
 */
export function derive_x25519(peerPubKey: Uint8Array): Uint8Array {
  if (!_privateKey) {
    throw new Error("Device identity not initialized. Call ensureIdentity() first.");
  }
  const xPriv = ed25519SeedToX25519Priv(_privateKey);
  return x25519.getSharedSecret(xPriv, peerPubKey);
}

/** Serialize the current in-memory identity to a SerializedIdentity. */
function serializeIdentity(): SerializedIdentity {
  if (!_privateKey || !_publicKey || !_fingerprint) {
    throw new Error("Device identity not initialized.");
  }
  return {
    privateKeyHex: uint8ArrayToHex(_privateKey),
    publicKeyHex: uint8ArrayToHex(_publicKey),
    fingerprint: _fingerprint,
  };
}

/**
 * Re-key the secure store entry with a different `requireAuthentication` value.
 * `expo-secure-store` does not support changing this flag on an existing entry,
 * so we must delete and re-create the entry.
 */
export async function setAuthenticationRequirement(
  requireAuthentication: boolean,
): Promise<void> {
  if (!_privateKey || !_publicKey || !_fingerprint) {
    throw new Error("Device identity not initialized. Call ensureIdentity() first.");
  }

  const data = serializeIdentity();
  await SecureStore.deleteItemAsync(STORAGE_KEY);
  await SecureStore.setItemAsync(STORAGE_KEY, JSON.stringify(data), {
    requireAuthentication,
  });
  _requireAuthentication = requireAuthentication;
}

/**
 * Clear the device identity from secure storage and reset in-memory state.
 * Does NOT call Spine unpair or flush outbox — the caller is responsible
 * for those operations before calling this function.
 */
export async function clearIdentity(): Promise<void> {
  try {
    await SecureStore.deleteItemAsync(STORAGE_KEY);
  } catch {
    // SecureStore.deleteItemAsync throws if the key doesn't exist on some platforms.
    // Swallow — we want idempotent behavior.
  }
  _privateKey = null;
  _publicKey = null;
  _fingerprint = null;
  _requireAuthentication = false;
}

/**
 * Full device reset: clears identity, calls Spine unpair, and flushes the outbox.
 */
export async function device_reset(): Promise<void> {
  await revokeCurrentDevice();
  await clearOutbox();
  useAppStore.getState().setUnpaired();
  await clearIdentity();
}
