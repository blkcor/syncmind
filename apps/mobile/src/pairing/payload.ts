import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex } from "@noble/hashes/utils.js";
import { base64UrlNoPadToBytes } from "./encoding";
import { validateCAFingerprintFormat } from "./tls-check";

const PAIRING_KIND = "syncmind-pairing";
const CLOCK_SKEW_MS = 60_000;
const UUID_V4_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export interface PairingPayload {
  v: number;
  kind: typeof PAIRING_KIND;
  session_id: string;
  spine_url: string;
  ca_fingerprint: string | null;
  pairing_token: string;
  expires_at: string;
  device_a_pubkey: string;
  device_a_fingerprint: string;
}

export type ValidationErrorCode =
  | "malformed_json"
  | "not_object"
  | "missing_field"
  | "desktop_upgrade_required"
  | "unsupported_version"
  | "wrong_kind"
  | "invalid_session_id"
  | "expired"
  | "invalid_expires_at"
  | "insecure_url"
  | "invalid_url"
  | "invalid_pubkey"
  | "invalid_fingerprint"
  | "fingerprint_mismatch"
  | "invalid_ca_fingerprint";

export interface ValidationError {
  code: ValidationErrorCode;
  field?: keyof PairingPayload;
}

export class PairingPayloadError extends Error {
  code: ValidationErrorCode;
  field?: keyof PairingPayload;

  constructor(error: ValidationError) {
    super(validationErrorMessage(error));
    this.name = "PairingPayloadError";
    this.code = error.code;
    this.field = error.field;
  }
}

export function parsePairingPayload(input: string): PairingPayload {
  let parsed: unknown;
  try {
    parsed = JSON.parse(input.trim());
  } catch {
    throw new PairingPayloadError({ code: "malformed_json" });
  }

  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new PairingPayloadError({ code: "not_object" });
  }

  const record = parsed as Record<string, unknown>;
  if (
    record.v === 1 &&
    record.kind === PAIRING_KIND &&
    !Object.prototype.hasOwnProperty.call(record, "session_id")
  ) {
    throw new PairingPayloadError({ code: "desktop_upgrade_required" });
  }

  for (const field of [
    "v",
    "kind",
    "session_id",
    "spine_url",
    "ca_fingerprint",
    "pairing_token",
    "expires_at",
    "device_a_pubkey",
    "device_a_fingerprint",
  ] as const) {
    if (!Object.prototype.hasOwnProperty.call(record, field)) {
      throw new PairingPayloadError({ code: "missing_field", field });
    }
  }

  if (record.kind !== PAIRING_KIND) {
    throw new PairingPayloadError({ code: "wrong_kind" });
  }

  return {
    v: expectNumber(record.v, "v"),
    kind: PAIRING_KIND,
    session_id: expectString(record.session_id, "session_id"),
    spine_url: expectString(record.spine_url, "spine_url"),
    ca_fingerprint: expectNullableString(record.ca_fingerprint, "ca_fingerprint"),
    pairing_token: expectString(record.pairing_token, "pairing_token"),
    expires_at: expectString(record.expires_at, "expires_at"),
    device_a_pubkey: expectString(record.device_a_pubkey, "device_a_pubkey"),
    device_a_fingerprint: expectString(
      record.device_a_fingerprint,
      "device_a_fingerprint",
    ),
  };
}

export function validatePairingPayload(
  payload: PairingPayload,
  options: { allowHttp?: boolean; now?: Date } = {},
): ValidationError | null {
  if (payload.v !== 1) {
    return { code: "unsupported_version" };
  }
  if (payload.kind !== PAIRING_KIND) {
    return { code: "wrong_kind", field: "kind" };
  }
  if (!UUID_V4_RE.test(payload.session_id)) {
    return { code: "invalid_session_id", field: "session_id" };
  }

  const expiresAtMs = Date.parse(payload.expires_at);
  if (Number.isNaN(expiresAtMs)) {
    return { code: "invalid_expires_at", field: "expires_at" };
  }
  const nowMs = options.now?.getTime() ?? Date.now();
  if (nowMs > expiresAtMs + CLOCK_SKEW_MS) {
    return { code: "expired", field: "expires_at" };
  }

  let url: URL;
  try {
    url = new URL(payload.spine_url);
  } catch {
    return { code: "invalid_url", field: "spine_url" };
  }
  if (url.protocol === "http:" && !options.allowHttp) {
    return { code: "insecure_url", field: "spine_url" };
  }
  if (url.protocol !== "https:" && url.protocol !== "http:") {
    return { code: "invalid_url", field: "spine_url" };
  }

  let pubkey: Uint8Array;
  try {
    pubkey = base64UrlNoPadToBytes(payload.device_a_pubkey);
  } catch {
    return { code: "invalid_pubkey", field: "device_a_pubkey" };
  }
  if (pubkey.length !== 32) {
    return { code: "invalid_pubkey", field: "device_a_pubkey" };
  }

  if (!/^sha256:[0-9a-f]{64}$/.test(payload.device_a_fingerprint)) {
    return { code: "invalid_fingerprint", field: "device_a_fingerprint" };
  }
  const expectedFingerprint = `sha256:${bytesToHex(sha256(pubkey))}`;
  if (payload.device_a_fingerprint !== expectedFingerprint) {
    return { code: "fingerprint_mismatch", field: "device_a_fingerprint" };
  }

  if (
    payload.ca_fingerprint !== null &&
    !validateCAFingerprintFormat(payload.ca_fingerprint)
  ) {
    return { code: "invalid_ca_fingerprint", field: "ca_fingerprint" };
  }

  return null;
}

export function validationErrorMessage(error: ValidationError): string {
  switch (error.code) {
    case "malformed_json":
    case "not_object":
      return "Invalid QR code - this doesn't look like a SyncMind pairing code";
    case "desktop_upgrade_required":
      return "Desktop version too old - update SyncMind Desktop and generate a new QR code";
    case "unsupported_version":
      return "App version too old - please update SyncMind Mobile and try again";
    case "expired":
      return "QR code expired - please generate a new one from the desktop Devices panel";
    case "insecure_url":
      return "Insecure connection - HTTPS is required";
    case "invalid_ca_fingerprint":
      return "Invalid certificate fingerprint in QR code";
    case "wrong_kind":
    case "missing_field":
    case "invalid_session_id":
    case "invalid_expires_at":
    case "invalid_url":
    case "invalid_pubkey":
    case "invalid_fingerprint":
    case "fingerprint_mismatch":
      return "Invalid QR code - this doesn't look like a SyncMind pairing code";
  }
}

function expectString(
  value: unknown,
  field: keyof PairingPayload,
): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new PairingPayloadError({ code: "missing_field", field });
  }
  return value;
}

function expectNullableString(
  value: unknown,
  field: keyof PairingPayload,
): string | null {
  if (value === null) {
    return null;
  }
  return expectString(value, field);
}

function expectNumber(
  value: unknown,
  field: keyof PairingPayload,
): number {
  if (typeof value !== "number") {
    throw new PairingPayloadError({ code: "missing_field", field });
  }
  return value;
}
