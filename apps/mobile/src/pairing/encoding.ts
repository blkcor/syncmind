export function bytesToBase64UrlNoPad(bytes: Uint8Array): string {
  const base64 = bytesToBase64(bytes);
  return base64.replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");
}

export function base64UrlNoPadToBytes(value: string): Uint8Array {
  if (!/^[A-Za-z0-9_-]+$/.test(value) || value.includes("=")) {
    throw new Error("Invalid base64url-no-pad value.");
  }

  const padLength = (4 - (value.length % 4)) % 4;
  const base64 = `${value.replaceAll("-", "+").replaceAll("_", "/")}${"=".repeat(
    padLength,
  )}`;
  return base64ToBytes(base64);
}

export function bytesToBase64(bytes: Uint8Array): string {
  if ("Buffer" in globalThis) {
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

export function base64ToBytes(base64: string): Uint8Array {
  if ("Buffer" in globalThis) {
    const buffer = (globalThis as typeof globalThis & {
      Buffer: { from(data: string, encoding: string): Uint8Array };
    }).Buffer;
    return new Uint8Array(buffer.from(base64, "base64"));
  }

  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}
