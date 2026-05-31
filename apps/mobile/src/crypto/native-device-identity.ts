import { requireOptionalNativeModule } from "expo-modules-core";

export interface DeviceIdentityMeta {
  fingerprint: string;
  publicKeyHex: string;
  biometricEnabled: boolean;
}

export interface NativeDeviceIdentityModule {
  ensureIdentity(): Promise<DeviceIdentityMeta>;
  getIdentityMeta(): Promise<DeviceIdentityMeta | null>;
  sign(messageBase64: string): Promise<string>;
  deriveX25519(peerPubKeyHex: string): Promise<string>;
  setBiometricProtection(enabled: boolean): Promise<void>;
  resetIdentity(): Promise<void>;
  importLegacyIdentity(privateKeyHex: string): Promise<DeviceIdentityMeta>;
}

const unavailableMessage =
  "SyncMindDeviceIdentity native module is unavailable. Use a development build or rebuilt native app; Expo Go cannot load this local native module.";

let loadedNativeDeviceIdentity: NativeDeviceIdentityModule | null = null;

function getNativeDeviceIdentity(): NativeDeviceIdentityModule {
  const nativeModule =
    loadedNativeDeviceIdentity ??
    requireOptionalNativeModule<NativeDeviceIdentityModule>(
      "SyncMindDeviceIdentity",
    );

  if (!nativeModule) {
    throw new Error(unavailableMessage);
  }

  loadedNativeDeviceIdentity = nativeModule;
  return nativeModule;
}

const NativeDeviceIdentity: NativeDeviceIdentityModule = {
  async ensureIdentity() {
    return getNativeDeviceIdentity().ensureIdentity();
  },
  async getIdentityMeta() {
    return getNativeDeviceIdentity().getIdentityMeta();
  },
  async sign(messageBase64: string) {
    return getNativeDeviceIdentity().sign(messageBase64);
  },
  async deriveX25519(peerPubKeyHex: string) {
    return getNativeDeviceIdentity().deriveX25519(peerPubKeyHex);
  },
  async setBiometricProtection(enabled: boolean) {
    return getNativeDeviceIdentity().setBiometricProtection(enabled);
  },
  async resetIdentity() {
    return getNativeDeviceIdentity().resetIdentity();
  },
  async importLegacyIdentity(privateKeyHex: string) {
    return getNativeDeviceIdentity().importLegacyIdentity(privateKeyHex);
  },
};

export default NativeDeviceIdentity;
