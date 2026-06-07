import type { ImagePickerAsset } from "expo-image-picker";
import {
  manipulateAsync as defaultManipulateAsync,
  SaveFormat,
  type Action,
  type ImageResult,
} from "expo-image-manipulator";
import { File } from "expo-file-system";
import {
  buildCaptureImageEnvelope,
  createCaptureImagePayload,
  secureSerialize,
} from "../crypto/bundle";

export const MAX_IMAGE_EDGE_PX = 2048;
export const MAX_IMAGE_ENCODED_BYTES = 5 * 1024 * 1024;
export const MAX_DECODED_BUNDLE_CONTENT_BYTES = 12 * 1024 * 1024;

const JPEG_QUALITY_PRIMARY = 0.85;
const JPEG_QUALITY_RETRY = 0.7;

export interface ImageFileLike {
  info(): { exists?: boolean; size?: number | null };
  base64(): Promise<string>;
  delete(): void;
}

export interface ImagePreprocessDeps {
  manipulateAsync?: (
    uri: string,
    actions: Action[],
    saveOptions: { compress: number; format: SaveFormat; base64: true },
  ) => Promise<ImageResult>;
  fileFromUri?: (uri: string) => ImageFileLike;
}

export type ImagePreprocessResult =
  | {
      status: "ready";
      imageBase64: string;
      imageMime: "image/jpeg";
      width: number;
      height: number;
      byteLength: number;
      quality: 0.85 | 0.7;
      exifPreservation: "picker-exif-read-manipulator-reencode-unsupported";
    }
  | { status: "image-too-large" }
  | { status: "preprocessing-failed" };

export interface CaptureImageSerializedSizeParams {
  id: string;
  imageBase64: string;
  width: number;
  height: number;
  caption: string | null;
  clientTs: string;
  clientDeviceFingerprint: string;
}

export type CaptureImageSerializedSizeValidation =
  | { ok: true; contentBytes: number; envelopeBytes: number }
  | { ok: false; reason: "image-too-large" };

export function calculateResize(width: number, height: number): {
  actions: Action[];
  width: number;
  height: number;
} {
  const longEdge = Math.max(width, height);
  if (longEdge <= MAX_IMAGE_EDGE_PX || width <= 0 || height <= 0) {
    return { actions: [], width, height };
  }

  const scale = MAX_IMAGE_EDGE_PX / longEdge;
  const resizedWidth = Math.round(width * scale);
  const resizedHeight = Math.round(height * scale);

  return {
    actions: [
      {
        resize: {
          width: resizedWidth,
          height: resizedHeight,
        },
      },
    ],
    width: resizedWidth,
    height: resizedHeight,
  };
}

export async function preprocessSelectedImage(
  asset: ImagePickerAsset,
  deps: ImagePreprocessDeps = {},
): Promise<ImagePreprocessResult> {
  const manipulateAsync = deps.manipulateAsync ?? defaultManipulateAsync;
  const fileFromUri =
    deps.fileFromUri ?? ((uri: string): ImageFileLike => new File(uri));
  const resize = calculateResize(asset.width, asset.height);

  try {
    const primary = await encodeJpegAttempt({
      assetUri: asset.uri,
      actions: resize.actions,
      quality: JPEG_QUALITY_PRIMARY,
      manipulateAsync,
      fileFromUri,
    });
    if (primary.status === "ready" && primary.byteLength <= MAX_IMAGE_ENCODED_BYTES) {
      return primary;
    }

    const retry = await encodeJpegAttempt({
      assetUri: asset.uri,
      actions: resize.actions,
      quality: JPEG_QUALITY_RETRY,
      manipulateAsync,
      fileFromUri,
    });
    if (retry.status === "ready" && retry.byteLength <= MAX_IMAGE_ENCODED_BYTES) {
      return retry;
    }

    return { status: "image-too-large" };
  } catch {
    return { status: "preprocessing-failed" };
  }
}

export function validateCaptureImageSerializedSize(
  params: CaptureImageSerializedSizeParams,
): CaptureImageSerializedSizeValidation {
  const payload = createCaptureImagePayload({
    id: params.id,
    image_base64: params.imageBase64,
    width: params.width,
    height: params.height,
    caption: params.caption,
    client_ts: params.clientTs,
    client_device_fingerprint: params.clientDeviceFingerprint,
  });
  const contentBytes = secureSerialize(payload).byteLength;
  if (contentBytes > MAX_DECODED_BUNDLE_CONTENT_BYTES) {
    return { ok: false, reason: "image-too-large" };
  }

  const envelope = buildCaptureImageEnvelope({
    id: params.id,
    image_base64: params.imageBase64,
    width: params.width,
    height: params.height,
    caption: params.caption,
    client_ts: params.clientTs,
    client_device_fingerprint: params.clientDeviceFingerprint,
  });
  const envelopeBytes = secureSerialize(envelope).byteLength;
  if (envelopeBytes > MAX_DECODED_BUNDLE_CONTENT_BYTES) {
    return { ok: false, reason: "image-too-large" };
  }

  return { ok: true, contentBytes, envelopeBytes };
}

async function encodeJpegAttempt(params: {
  assetUri: string;
  actions: Action[];
  quality: 0.85 | 0.7;
  manipulateAsync: NonNullable<ImagePreprocessDeps["manipulateAsync"]>;
  fileFromUri: NonNullable<ImagePreprocessDeps["fileFromUri"]>;
}): Promise<ImagePreprocessResult> {
  const result = await params.manipulateAsync(params.assetUri, params.actions, {
    compress: params.quality,
    format: SaveFormat.JPEG,
    base64: true,
  });
  const file = params.fileFromUri(result.uri);
  try {
    const info = file.info();
    const imageBase64 = result.base64 ?? await file.base64();
    const byteLength =
      typeof info.size === "number" ? info.size : estimateDecodedBase64Bytes(imageBase64);

    return {
      status: "ready",
      imageBase64,
      imageMime: "image/jpeg",
      width: result.width,
      height: result.height,
      byteLength,
      quality: params.quality,
      exifPreservation: "picker-exif-read-manipulator-reencode-unsupported",
    };
  } finally {
    try {
      file.delete();
    } catch {
      // Best-effort temp cleanup; callers still decide whether to enqueue.
    }
  }
}

function estimateDecodedBase64Bytes(base64: string): number {
  const padding = base64.endsWith("==") ? 2 : base64.endsWith("=") ? 1 : 0;
  return Math.floor(base64.length * 3 / 4) - padding;
}
