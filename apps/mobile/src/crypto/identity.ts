import * as SecureStore from "expo-secure-store";
import { clearOutbox } from "../outbox/service";
import { revokeCurrentDevice } from "../spine/client";
import { useAppStore } from "../store";
import NativeDeviceIdentity, {
  type DeviceIdentityMeta,
} from "./native-device-identity";

const LEGACY_STORAGE_KEY = "device_identity";
const META_STORAGE_KEY = "device_identity_meta";

interface LegacySerializedIdentity {
  privateKeyHex: string;
  publicKeyHex: string;
  fingerprint: string;
}

let _publicKey: Uint8Array | null = null;
let _fingerprint: string | null = null;
let _requireAuthentication = false;

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) {
    throw new Error("Invalid hex payload.");
  }

  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    const byte = Number.parseInt(hex.slice(i, i + 2), 16);
    if (Number.isNaN(byte)) {
      throw new Error("Invalid hex payload.");
    }
    bytes[i / 2] = byte;
  }
  return bytes;
}

function hydrateIdentity(meta: DeviceIdentityMeta): void {
  _publicKey = hexToBytes(meta.publicKeyHex);
  _fingerprint = meta.fingerprint;
  _requireAuthentication = meta.biometricEnabled;
}

function clearIdentityCache(): void {
  _publicKey = null;
  _fingerprint = null;
  _requireAuthentication = false;
}

async function persistIdentityMeta(meta: DeviceIdentityMeta): Promise<void> {
  await SecureStore.setItemAsync(META_STORAGE_KEY, JSON.stringify(meta));
}

async function tryMigrateLegacyIdentity(): Promise<DeviceIdentityMeta | null> {
  const stored = await SecureStore.getItemAsync(LEGACY_STORAGE_KEY);
  if (!stored) {
    return null;
  }

  const legacy = JSON.parse(stored) as LegacySerializedIdentity;
  try {
    const imported = await NativeDeviceIdentity.importLegacyIdentity(legacy.privateKeyHex);
    await SecureStore.deleteItemAsync(LEGACY_STORAGE_KEY);
    await persistIdentityMeta(imported);
    return imported;
  } catch {
    throw new Error("Unable to migrate legacy device identity. Reset identity to continue.");
  }
}

/**
 * Ensure a device identity exists, generating one if necessary.
 * Returns the device fingerprint string (`sha256:<hex>`).
 */
export async function ensureIdentity(): Promise<string> {
  if (_publicKey && _fingerprint) {
    return _fingerprint;
  }

  let meta = await NativeDeviceIdentity.getIdentityMeta();

  if (!meta) {
    meta = await tryMigrateLegacyIdentity();
  }

  if (!meta) {
    meta = await NativeDeviceIdentity.ensureIdentity();
  }

  hydrateIdentity(meta);
  await persistIdentityMeta(meta);

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

function bytesToBase64(bytes: Uint8Array): string {
  if (typeof btoa !== "function" && "Buffer" in globalThis) {
    const buffer = (globalThis as typeof globalThis & {
      Buffer: { from(data: Uint8Array): { toString(encoding: string): string } };
    }).Buffer;
    return buffer.from(bytes).toString("base64");
  }

  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}

function base64ToBytes(base64: string): Uint8Array {
  if (typeof atob !== "function" && "Buffer" in globalThis) {
    const buffer = (globalThis as typeof globalThis & {
      Buffer: { from(data: string, encoding: string): Uint8Array };
    }).Buffer;
    return new Uint8Array(buffer.from(base64, "base64"));
  }

  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/**
 * Sign a message with the device's native private key.
 * Returns the raw signature bytes.
 */
export async function sign(message: Uint8Array): Promise<Uint8Array> {
  if (!_fingerprint) {
    throw new Error("Device identity not initialized. Call ensureIdentity() first.");
  }

  const encodedMessage = bytesToBase64(message);
  const result = await NativeDeviceIdentity.sign(encodedMessage);
  return base64ToBytes(result);
}

/**
 * Derive an X25519 shared secret using the native identity.
 */
export async function derive_x25519(peerPubKey: Uint8Array): Promise<Uint8Array> {
  if (!_fingerprint) {
    throw new Error("Device identity not initialized. Call ensureIdentity() first.");
  }

  const result = await NativeDeviceIdentity.deriveX25519(bytesToHex(peerPubKey));
  return base64ToBytes(result);
}

export async function setAuthenticationRequirement(
  requireAuthentication: boolean,
): Promise<void> {
  if (!_fingerprint) {
    throw new Error("Device identity not initialized. Call ensureIdentity() first.");
  }

  await NativeDeviceIdentity.setBiometricProtection(requireAuthentication);
  _requireAuthentication = requireAuthentication;
  await persistIdentityMeta({
    fingerprint: _fingerprint,
    publicKeyHex: bytesToHex(getDevicePubkey()),
    biometricEnabled: requireAuthentication,
  });
}

/**
 * Clear the native device identity and cached public metadata.
 */
export async function clearIdentity(): Promise<void> {
  try {
    await NativeDeviceIdentity.resetIdentity();
  } finally {
    await SecureStore.deleteItemAsync(META_STORAGE_KEY);
    clearIdentityCache();
  }
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

export function __resetIdentityCacheForTests(): void {
  clearIdentityCache();
}
