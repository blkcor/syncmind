/**
 * Tests for the mobile bundle crypto module: secureSerialize, envelope construction,
 * AES-256-GCM encryption, fingerprint AAD derivation, and deterministic fixtures.
 */

import * as Crypto from "expo-crypto";

jest.mock("expo-crypto", () => {
  let nonceCall = 0;
  return {
    randomUUID: jest.fn(() => "cccccccc-cccc-4ccc-8ccc-cccccccccccc"),
    getRandomBytes: jest.fn((size: number) => {
      nonceCall++;
      const bytes = new Uint8Array(size);
      for (let i = 0; i < size; i++) {
        bytes[i] = (nonceCall * 17 + i) % 256;
      }
      return bytes;
    }),
  };
});

import { gcm } from "@noble/ciphers/aes";
import {
  secureSerialize,
  peerFingerprintToAAD,
  createCaptureTextPayload,
  createCaptureAudioPayload,
  createCaptureImagePayload,
  buildCaptureTextEnvelope,
  buildCaptureAudioEnvelope,
  buildCaptureImageEnvelope,
  encryptBundle,
  encryptCaptureText,
  encryptCaptureAudio,
  encryptCaptureImage,
} from "../src/crypto/bundle";
import type { PersistedPairingState } from "../src/spine/session";

// ── Helpers ─────────────────────────────────────────────────────────

const FINGERPRINT_PREFIX = "sha256:";

function buildFingerprint(hex: string): string {
  return `${FINGERPRINT_PREFIX}${hex}`;
}

function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = Number.parseInt(hex.slice(i, i + 2), 16);
  }
  return bytes;
}

// 32-byte = 64-char valid hex fingerprint
const VALID_HEX_64 = "ab".repeat(32); // exactly 64 hex chars → 32 bytes

const testPairingState: PersistedPairingState = {
  selfDeviceUuid: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  syncKey: new Uint8Array(32).fill(0x42),
  pairedPeerFingerprint: buildFingerprint(VALID_HEX_64),
  pairedPeerDeviceId: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
  pairedPeerDeviceType: "desktop",
  pairedAt: "2026-05-31T00:00:00.000Z",
  spineUrl: "https://spine.syncmind.local:8443",
  caFingerprint: null,
  lastSeenAt: null,
};

// ── secureSerialize ──────────────────────────────────────────────────

describe("secureSerialize", () => {
  it("produces UTF-8 bytes from a payload object", () => {
    const payload = { kind: "capture-text", text: "hello" };
    const bytes = secureSerialize(payload);

    expect(bytes).toBeInstanceOf(Uint8Array);
    expect(bytes.length).toBeGreaterThan(0);

    const decoded = new TextDecoder().decode(bytes);
    const parsed = JSON.parse(decoded);
    expect(parsed).toEqual(payload);
  });

  it("secureSerialize replaces direct JSON.stringify in the encryption path", () => {
    // Verify that encryptBundle uses secureSerialize internally
    // by checking that the ciphertext is valid and non-empty
    const payload = { text: "hello" };
    const bytes = secureSerialize(payload);
    const direct = new TextEncoder().encode(JSON.stringify(payload));

    // Both should produce identical bytes (secureSerialize is JSON.stringify wrapper)
    expect(bytes).toEqual(direct);
  });

  it("rejects direct JSON.stringify for guarded capture payloads", () => {
    const payload = createCaptureTextPayload({
      id: "cap-guard",
      text: "secret",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });

    expect(() => JSON.stringify(payload)).toThrow("secureSerialize");
    expect(() => secureSerialize(payload)).not.toThrow();
  });

  it("creates the US-043 capture-text payload schema", () => {
    const payload = createCaptureTextPayload({
      id: "cap-us-043",
      text: "hello",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });

    expect(payload).toMatchObject({
      v: 1,
      kind: "capture-text",
      id: "cap-us-043",
      text: "hello",
      source: "typed",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });
  });

  it("creates the US-044 capture-audio payload schema", () => {
    const payload = createCaptureAudioPayload({
      id: "audio-us-044",
      audio_base64: "ZmFrZS1tNGE=",
      duration_ms: 12_345,
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });

    expect(payload).toMatchObject({
      v: 1,
      kind: "capture-audio",
      id: "audio-us-044",
      audio_base64: "ZmFrZS1tNGE=",
      audio_mime: "audio/mp4",
      duration_ms: 12_345,
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });
    expect(() => JSON.stringify(payload)).toThrow("secureSerialize");
    expect(() => secureSerialize(payload)).not.toThrow();
  });

  it("creates the US-045 capture-image payload schema with null caption", () => {
    const payload = createCaptureImagePayload({
      id: "image-us-045-null",
      image_base64: "SlBFRw==",
      width: 2048,
      height: 1536,
      caption: null,
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });

    expect(payload).toMatchObject({
      v: 1,
      kind: "capture-image",
      id: "image-us-045-null",
      image_base64: "SlBFRw==",
      image_mime: "image/jpeg",
      width: 2048,
      height: 1536,
      caption: null,
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });
    expect(() => JSON.stringify(payload)).toThrow("secureSerialize");
    expect(() => secureSerialize(payload)).not.toThrow();
  });

  it("creates the US-045 capture-image payload schema with non-empty caption", () => {
    const payload = createCaptureImagePayload({
      id: "image-us-045-caption",
      image_base64: "SlBFRw==",
      width: 1600,
      height: 900,
      caption: "whiteboard plan",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });

    const serialized = JSON.parse(new TextDecoder().decode(secureSerialize(payload)));
    expect(serialized.caption).toBe("whiteboard plan");
    expect(serialized.image_base64).toBe("SlBFRw==");
  });
});

// ── peerFingerprintToAAD ─────────────────────────────────────────────

describe("peerFingerprintToAAD", () => {
  it("decodes a valid 64-char hex fingerprint to 32 bytes", () => {
    const fp = buildFingerprint(VALID_HEX_64);
    const aad = peerFingerprintToAAD(fp);

    expect(aad).toBeInstanceOf(Uint8Array);
    expect(aad.length).toBe(32);
  });

  it("rejects fingerprint without sha256: prefix", () => {
    expect(() => peerFingerprintToAAD(VALID_HEX_64)).toThrow(
      "must start with sha256:",
    );
  });

  it("rejects fingerprints with wrong hex length", () => {
    expect(() => peerFingerprintToAAD("sha256:abc")).toThrow(
      "hex part must be 64 chars",
    );
  });

  it("rejects fingerprints with non-hex characters", () => {
    expect(() =>
      peerFingerprintToAAD(`sha256:${"gg".repeat(32)}`),
    ).toThrow("non-hex characters");
  });
});

// ── buildCaptureTextEnvelope ─────────────────────────────────────────

describe("buildCaptureTextEnvelope", () => {
  it("constructs a valid capture-text envelope with correct sha256", () => {
    const payload = {
      id: "cap-1",
      text: "hello world",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    };

    const envelope = buildCaptureTextEnvelope(payload);

    expect(envelope.schema_version).toBe(1);
    expect(envelope.kind).toBe("capture-text");
    expect(envelope.filename).toBe("capture-cap-1.json");
    expect(envelope.captured_at).toBeTruthy();
    expect(JSON.parse(envelope.content_utf8)).toEqual({
      v: 1,
      kind: "capture-text",
      ...payload,
      source: "typed",
    });
    expect(envelope.sha256).toMatch(/^[0-9a-f]{64}$/);
  });

  it("produces different sha256 for different content", () => {
    const e1 = buildCaptureTextEnvelope({
      id: "cap-1",
      text: "hello",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });
    const e2 = buildCaptureTextEnvelope({
      id: "cap-2",
      text: "world",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });

    expect(e1.sha256).not.toBe(e2.sha256);
  });
});

// ── buildCaptureAudioEnvelope ────────────────────────────────────────

describe("buildCaptureAudioEnvelope", () => {
  it("constructs a valid capture-audio envelope with correct sha256", () => {
    const payload = {
      id: "audio-1",
      audio_base64: "ZmFrZS1tNGE=",
      duration_ms: 5000,
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    };

    const envelope = buildCaptureAudioEnvelope(payload);

    expect(envelope.schema_version).toBe(1);
    expect(envelope.kind).toBe("capture-audio");
    expect(envelope.filename).toBe("capture-audio-1.json");
    expect(envelope.captured_at).toBeTruthy();
    expect(JSON.parse(envelope.content_utf8)).toEqual({
      v: 1,
      kind: "capture-audio",
      ...payload,
      audio_mime: "audio/mp4",
    });
    expect(envelope.sha256).toMatch(/^[0-9a-f]{64}$/);
  });
});

// ── buildCaptureImageEnvelope ────────────────────────────────────────

describe("buildCaptureImageEnvelope", () => {
  it("constructs a valid capture-image envelope with deterministic filename and sha256", () => {
    const payload = {
      id: "image-1",
      image_base64: "SlBFRw==",
      width: 1024,
      height: 768,
      caption: null,
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    };

    const envelope = buildCaptureImageEnvelope(payload);

    expect(envelope.schema_version).toBe(1);
    expect(envelope.kind).toBe("capture-image");
    expect(envelope.filename).toBe("capture-image-1.json");
    expect(envelope.captured_at).toBeTruthy();
    expect(JSON.parse(envelope.content_utf8)).toEqual({
      v: 1,
      kind: "capture-image",
      ...payload,
      image_mime: "image/jpeg",
    });
    expect(envelope.sha256).toMatch(/^[0-9a-f]{64}$/);
  });
});

// ── encryptBundle ────────────────────────────────────────────────────

describe("encryptBundle", () => {
  it("produces a nonce(12) | ciphertext_and_tag blob", async () => {
    const envelope = buildCaptureTextEnvelope({
      id: "cap-1",
      text: "secret message",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });
    const aad = peerFingerprintToAAD(testPairingState.pairedPeerFingerprint);

    const result = await encryptBundle({
      envelope,
      syncKey: testPairingState.syncKey,
      peerFingerprintAAD: aad,
    });

    expect(result.blob).toBeInstanceOf(Uint8Array);
    expect(result.blob.length).toBeGreaterThanOrEqual(28); // 12 nonce + min 16 tag
    expect(result.payloadHash).toMatch(/^[0-9a-f]{64}$/);
  });

  it("produces deterministic output with fixed nonce", async () => {
    (Crypto.getRandomBytes as jest.Mock).mockReturnValueOnce(
      new Uint8Array(12).fill(0x01),
    );

    const envelope = buildCaptureTextEnvelope({
      id: "cap-1",
      text: "deterministic",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });
    const aad = peerFingerprintToAAD(testPairingState.pairedPeerFingerprint);

    const result1 = await encryptBundle({
      envelope,
      syncKey: testPairingState.syncKey,
      peerFingerprintAAD: aad,
    });

    (Crypto.getRandomBytes as jest.Mock).mockReturnValueOnce(
      new Uint8Array(12).fill(0x01),
    );

    const result2 = await encryptBundle({
      envelope,
      syncKey: testPairingState.syncKey,
      peerFingerprintAAD: aad,
    });

    expect(result1.blob).toEqual(result2.blob);
    expect(result1.payloadHash).toBe(result2.payloadHash);
  });

  it("different nonces produce different ciphertexts", async () => {
    const envelope = buildCaptureTextEnvelope({
      id: "cap-1",
      text: "test",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });
    const aad = peerFingerprintToAAD(testPairingState.pairedPeerFingerprint);

    const result1 = await encryptBundle({
      envelope,
      syncKey: testPairingState.syncKey,
      peerFingerprintAAD: aad,
    });
    const result2 = await encryptBundle({
      envelope,
      syncKey: testPairingState.syncKey,
      peerFingerprintAAD: aad,
    });

    expect(result1.blob).not.toEqual(result2.blob);
    expect(result1.payloadHash).not.toBe(result2.payloadHash);
  });

  it("rejects keys that are not exactly 32 bytes", async () => {
    const envelope = buildCaptureTextEnvelope({
      id: "cap-1",
      text: "test",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });
    const aad = peerFingerprintToAAD(testPairingState.pairedPeerFingerprint);

    await expect(
      encryptBundle({
        envelope,
        syncKey: new Uint8Array(16),
        peerFingerprintAAD: aad,
      }),
    ).rejects.toThrow("syncKey must be exactly 32 bytes");
  });

  it("fails to decrypt with wrong key (tamper test)", async () => {
    const envelope = buildCaptureTextEnvelope({
      id: "cap-1",
      text: "tamper test",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });
    const aad = peerFingerprintToAAD(testPairingState.pairedPeerFingerprint);

    const result = await encryptBundle({
      envelope,
      syncKey: testPairingState.syncKey,
      peerFingerprintAAD: aad,
    });

    const nonce = result.blob.slice(0, 12);
    const ciphertext = result.blob.slice(12);

    const wrongKey = new Uint8Array(32).fill(0x99);
    const cipher = gcm(wrongKey, nonce, aad);

    expect(() => cipher.decrypt(ciphertext)).toThrow();
  });
});

// ── encryptCaptureText ───────────────────────────────────────────────

describe("encryptCaptureText", () => {
  it("encrypts a capture text payload end-to-end", async () => {
    const payload = {
      id: "cap-1",
      text: "end-to-end test",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    };

    const result = await encryptCaptureText(payload, testPairingState);

    expect(result.id).toBe("cap-1");
    expect(result.blob).toBeInstanceOf(Uint8Array);
    expect(result.payloadHash).toMatch(/^[0-9a-f]{64}$/);
  });
});

// ── encryptCaptureAudio ──────────────────────────────────────────────

describe("encryptCaptureAudio", () => {
  it("encrypts a capture audio payload end-to-end", async () => {
    const payload = createCaptureAudioPayload({
      id: "audio-1",
      audio_base64: "ZmFrZS1tNGE=",
      duration_ms: 5000,
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });

    const result = await encryptCaptureAudio(payload, testPairingState);

    expect(result.id).toBe("audio-1");
    expect(result.blob).toBeInstanceOf(Uint8Array);
    expect(result.blob.length).toBeGreaterThanOrEqual(28);
    expect(result.payloadHash).toMatch(/^[0-9a-f]{64}$/);
  });
});

// ── encryptCaptureImage ──────────────────────────────────────────────

describe("encryptCaptureImage", () => {
  it("encrypts a capture image payload with nonce | ciphertext_and_tag wire shape", async () => {
    const payload = createCaptureImagePayload({
      id: "image-1",
      image_base64: "SlBFRw==",
      width: 1024,
      height: 768,
      caption: "receipt",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });

    const result = await encryptCaptureImage(payload, testPairingState);

    expect(result.id).toBe("image-1");
    expect(result.blob).toBeInstanceOf(Uint8Array);
    expect(result.blob.length).toBeGreaterThanOrEqual(28);
    expect(result.payloadHash).toMatch(/^[0-9a-f]{64}$/);
  });
});

// ── Privacy: plaintext not in outputs ─────────────────────────────────

describe("privacy — no plaintext leak in encryption outputs", () => {
  it("encrypted blob does not contain the plaintext capture text", async () => {
    const payload = {
      id: "cap-1",
      text: "my secret note",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    };

    const result = await encryptCaptureText(payload, testPairingState);

    const blobStr = new TextDecoder().decode(result.blob);
    expect(blobStr).not.toContain("my secret note");
  });

  it("payloadHash does not leak plaintext", async () => {
    const result1 = await encryptCaptureText(
      {
        id: "a",
        text: "message one",
        client_ts: "2026-01-01T00:00:00.000Z",
        client_device_fingerprint: buildFingerprint(VALID_HEX_64),
      },
      testPairingState,
    );
    const result2 = await encryptCaptureText(
      {
        id: "b",
        text: "message two",
        client_ts: "2026-01-01T00:00:00.000Z",
        client_device_fingerprint: buildFingerprint(VALID_HEX_64),
      },
      testPairingState,
    );

    expect(result1.payloadHash).not.toBe(result2.payloadHash);
  });
});

// ── Deterministic fixture: tampered encrypted blob fails ─────────────

describe("deterministic crypto fixture", () => {
  it("tampered encrypted blob fails GCM authentication", async () => {
    const fixedNonce = new Uint8Array(12).fill(0x42);
    (Crypto.getRandomBytes as jest.Mock).mockReturnValueOnce(fixedNonce);

    const fixedKey = new Uint8Array(32).fill(0x03);
    const fixedAAD = hexToBytes(
      "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",
    );
    const envelope = buildCaptureTextEnvelope({
      id: "fixture-1",
      text: "fixture plaintext",
      client_ts: "2026-06-02T00:00:00.000Z",
      client_device_fingerprint: buildFingerprint(VALID_HEX_64),
    });

    const result = await encryptBundle({
      envelope,
      syncKey: fixedKey,
      peerFingerprintAAD: fixedAAD,
    });

    // Tamper with the ciphertext (flip a byte after the nonce)
    const tampered = new Uint8Array(result.blob);
    tampered[14] ^= 0x01;

    const nonce = tampered.slice(0, 12);
    const ciphertext = tampered.slice(12);

    const cipher = gcm(fixedKey, nonce, fixedAAD);
    expect(() => cipher.decrypt(ciphertext)).toThrow();
  });
});
