import * as Crypto from "expo-crypto";
import * as SecureStore from "expo-secure-store";

export const MOBILE_DEVICE_UUID_KEY = "syncmind.pairing.self_device_uuid";

export async function ensureMobileDeviceUuid(): Promise<string> {
  const existing = await getMobileDeviceUuid();
  if (existing) {
    return existing;
  }

  const generated = Crypto.randomUUID();
  await SecureStore.setItemAsync(MOBILE_DEVICE_UUID_KEY, generated);
  return generated;
}

export async function getMobileDeviceUuid(): Promise<string | null> {
  return SecureStore.getItemAsync(MOBILE_DEVICE_UUID_KEY);
}

export async function setMobileDeviceUuid(deviceUuid: string): Promise<void> {
  await SecureStore.setItemAsync(MOBILE_DEVICE_UUID_KEY, deviceUuid);
}

export async function clearMobileDeviceUuid(): Promise<void> {
  await SecureStore.deleteItemAsync(MOBILE_DEVICE_UUID_KEY);
}
