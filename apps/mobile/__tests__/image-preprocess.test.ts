import { SaveFormat } from "expo-image-manipulator";

import {
  MAX_IMAGE_ENCODED_BYTES,
  MAX_IMAGE_EDGE_PX,
  calculateResize,
  preprocessSelectedImage,
  validateCaptureImageSerializedSize,
  type ImageFileLike,
  type ImagePreprocessDeps,
} from "../src/capture/image";

function makeAsset(params: {
  uri?: string;
  width: number;
  height: number;
  mimeType?: string;
}) {
  return {
    uri: params.uri ?? "file:///tmp/original.heic",
    width: params.width,
    height: params.height,
    type: "image" as const,
    mimeType: params.mimeType ?? "image/heic",
    exif: { DateTimeOriginal: "2026:06:02 10:00:00" },
  };
}

function makeFile(size: number, base64 = "SlBFRw=="): ImageFileLike {
  return {
    info: jest.fn(() => ({ exists: true, size })),
    base64: jest.fn(async () => base64),
    delete: jest.fn(),
  };
}

function makeDeps(
  files: ImageFileLike[],
  results: Array<{ uri: string; width: number; height: number; base64?: string }>,
): ImagePreprocessDeps {
  return {
    manipulateAsync: jest.fn(async () => results.shift()!),
    fileFromUri: jest.fn(() => files.shift()!),
  };
}

describe("calculateResize", () => {
  it("resizes width proportionally when width exceeds 2048 px", () => {
    expect(calculateResize(4096, 2048)).toEqual({
      actions: [{ resize: { width: MAX_IMAGE_EDGE_PX, height: 1024 } }],
      width: 2048,
      height: 1024,
    });
  });

  it("resizes height proportionally when height exceeds 2048 px", () => {
    expect(calculateResize(1536, 4096)).toEqual({
      actions: [{ resize: { width: 768, height: MAX_IMAGE_EDGE_PX } }],
      width: 768,
      height: 2048,
    });
  });

  it("does not upscale images within 2048 px", () => {
    expect(calculateResize(1600, 900)).toEqual({
      actions: [],
      width: 1600,
      height: 900,
    });
  });
});

describe("preprocessSelectedImage", () => {
  it("normalizes non-JPEG input to image/jpeg at quality 85", async () => {
    const file = makeFile(128);
    const deps = makeDeps(
      [file],
      [{ uri: "file:///tmp/processed.jpg", width: 1600, height: 900, base64: "SlBFRw==" }],
    );

    const result = await preprocessSelectedImage(makeAsset({ width: 1600, height: 900 }), deps);

    expect(result).toMatchObject({
      status: "ready",
      imageMime: "image/jpeg",
      width: 1600,
      height: 900,
      byteLength: 128,
      quality: 0.85,
    });
    expect(deps.manipulateAsync).toHaveBeenCalledWith(
      "file:///tmp/original.heic",
      [],
      { compress: 0.85, format: SaveFormat.JPEG, base64: true },
    );
  });

  it("uses quality 85 output when it is within the 5 MB cap", async () => {
    const deps = makeDeps(
      [makeFile(MAX_IMAGE_ENCODED_BYTES)],
      [{ uri: "file:///tmp/q85.jpg", width: 2048, height: 1536, base64: "SlBFRw==" }],
    );

    const result = await preprocessSelectedImage(makeAsset({ width: 2048, height: 1536 }), deps);

    expect(result.status).toBe("ready");
    expect(deps.manipulateAsync).toHaveBeenCalledTimes(1);
  });

  it("retries quality 70 after quality 85 exceeds the 5 MB cap", async () => {
    const firstFile = makeFile(MAX_IMAGE_ENCODED_BYTES + 1);
    const retryFile = makeFile(MAX_IMAGE_ENCODED_BYTES);
    const deps = makeDeps(
      [firstFile, retryFile],
      [
        { uri: "file:///tmp/q85.jpg", width: 2048, height: 1536, base64: "q85" },
        { uri: "file:///tmp/q70.jpg", width: 2048, height: 1536, base64: "q70" },
      ],
    );

    const result = await preprocessSelectedImage(makeAsset({ width: 3000, height: 2250 }), deps);

    expect(result).toMatchObject({
      status: "ready",
      quality: 0.7,
      imageBase64: "q70",
    });
    expect(deps.manipulateAsync).toHaveBeenNthCalledWith(
      2,
      "file:///tmp/original.heic",
      [{ resize: { width: 2048, height: 1536 } }],
      { compress: 0.7, format: SaveFormat.JPEG, base64: true },
    );
    expect(firstFile.delete).toHaveBeenCalled();
  });

  it("rejects and deletes temp files when quality 70 still exceeds 5 MB", async () => {
    const firstFile = makeFile(MAX_IMAGE_ENCODED_BYTES + 1);
    const retryFile = makeFile(MAX_IMAGE_ENCODED_BYTES + 1);
    const deps = makeDeps(
      [firstFile, retryFile],
      [
        { uri: "file:///tmp/q85.jpg", width: 2048, height: 1536, base64: "q85" },
        { uri: "file:///tmp/q70.jpg", width: 2048, height: 1536, base64: "q70" },
      ],
    );

    await expect(
      preprocessSelectedImage(makeAsset({ width: 3000, height: 2250 }), deps),
    ).resolves.toEqual({ status: "image-too-large" });
    expect(firstFile.delete).toHaveBeenCalled();
    expect(retryFile.delete).toHaveBeenCalled();
  });

  it("rejects a processed image whose serialized capture payload exceeds the decoded bundle cap", async () => {
    const oversized = validateCaptureImageSerializedSize({
      imageBase64: "a".repeat(13 * 1024 * 1024),
      width: 2048,
      height: 1536,
      caption: null,
      clientDeviceFingerprint: "sha256:" + "ab".repeat(32),
      clientTs: "2026-06-02T00:00:00.000Z",
      id: "image-too-large",
    });

    expect(oversized).toEqual({
      ok: false,
      reason: "image-too-large",
    });
  });

  it("records the SDK 56 EXIF re-encode limitation instead of claiming preservation", async () => {
    const deps = makeDeps(
      [makeFile(128)],
      [{ uri: "file:///tmp/processed.jpg", width: 1600, height: 900, base64: "SlBFRw==" }],
    );

    const result = await preprocessSelectedImage(makeAsset({ width: 1600, height: 900 }), deps);

    expect(result).toMatchObject({
      status: "ready",
      exifPreservation: "picker-exif-read-manipulator-reencode-unsupported",
    });
  });
});
