const SHA256_FINGERPRINT_RE = /^sha256:[0-9a-fA-F]{64}$/;

export function validateCAFingerprintFormat(expected: string): boolean {
  return SHA256_FINGERPRINT_RE.test(expected);
}

export async function verifyPresentedCertificateFingerprint(
  expected: string | null,
): Promise<void> {
  if (!expected) {
    return;
  }

  // eslint-disable-next-line no-console
  console.warn("CA fingerprint validation skipped - not available in this environment");
}
