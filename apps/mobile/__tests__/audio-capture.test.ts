import {
  AUDIO_RECORDING_OPTIONS,
  MAX_AUDIO_BASE64_CHARS,
  MAX_AUDIO_RAW_BYTES,
  MAX_RECORDING_MS,
  createAudioCaptureSession,
  validateAudioSize,
} from "../src/capture/audio";

function makeRecorder() {
  return {
    uri: "file:///tmp/audio.m4a",
    prepareToRecordAsync: jest.fn(async () => {}),
    record: jest.fn(),
    stop: jest.fn(async () => {}),
    getStatus: jest.fn(() => ({
      canRecord: true,
      isRecording: true,
      durationMillis: 1234,
      mediaServicesDidReset: false,
      metering: -14,
      url: "file:///tmp/audio.m4a",
    })),
  };
}

function makeFile(params: {
  bytes?: Uint8Array;
  base64?: string;
  exists?: boolean;
  size?: number;
} = {}) {
  const bytes = params.bytes ?? new Uint8Array([1, 2, 3, 4]);
  const base64 = params.base64 ?? "AQIDBA==";
  return {
    bytes: jest.fn(async () => bytes),
    base64: jest.fn(async () => base64),
    info: jest.fn(() => ({
      exists: params.exists ?? true,
      size: params.size ?? bytes.byteLength,
    })),
    delete: jest.fn(),
  };
}

describe("audio capture recording options", () => {
  it("targets US-044 m4a AAC LC profile with metering and 60s cap", () => {
    expect(MAX_RECORDING_MS).toBe(60_000);
    expect(AUDIO_RECORDING_OPTIONS).toMatchObject({
      extension: ".m4a",
      sampleRate: 16_000,
      numberOfChannels: 1,
      bitRate: 32_000,
      isMeteringEnabled: true,
      android: {
        outputFormat: "mpeg4",
        audioEncoder: "aac",
        sampleRate: 16_000,
        extension: ".m4a",
      },
      ios: {
        sampleRate: 16_000,
        extension: ".m4a",
      },
      web: {
        mimeType: "audio/mp4",
        bitsPerSecond: 32_000,
      },
    });
  });
});

describe("validateAudioSize", () => {
  it("accepts audio within raw and base64 limits", () => {
    expect(validateAudioSize(MAX_AUDIO_RAW_BYTES, "a".repeat(64))).toEqual({
      ok: true,
    });
  });

  it("rejects audio over raw byte cap", () => {
    expect(validateAudioSize(MAX_AUDIO_RAW_BYTES + 1, "a")).toEqual({
      ok: false,
      reason: "clip-too-long",
    });
  });

  it("rejects audio over base64 cap", () => {
    expect(validateAudioSize(1, "a".repeat(MAX_AUDIO_BASE64_CHARS + 1))).toEqual({
      ok: false,
      reason: "clip-too-long",
    });
  });
});

describe("createAudioCaptureSession", () => {
  it("does not start recording when microphone permission is denied", async () => {
    const recorder = makeRecorder();
    const session = createAudioCaptureSession({
      recorder,
      requestPermissions: jest.fn(async () => ({ granted: false })),
      fileFromUri: jest.fn(makeFile),
    });

    await expect(session.start()).resolves.toEqual({
      status: "permission-denied",
    });
    expect(recorder.prepareToRecordAsync).not.toHaveBeenCalled();
    expect(recorder.record).not.toHaveBeenCalled();
  });

  it("prepares and starts recording with the configured max duration", async () => {
    const recorder = makeRecorder();
    const session = createAudioCaptureSession({
      recorder,
      requestPermissions: jest.fn(async () => ({ granted: true })),
      fileFromUri: jest.fn(makeFile),
    });

    await expect(session.start()).resolves.toMatchObject({
      status: "recording",
      metering: -14,
    });
    expect(recorder.prepareToRecordAsync).toHaveBeenCalledWith();
    expect(recorder.record).toHaveBeenCalledWith({ forDuration: 60 });
  });

  it("stops, reads bytes/base64, returns clip metadata, and deletes temp file", async () => {
    const recorder = makeRecorder();
    const file = makeFile({ bytes: new Uint8Array([9, 8, 7]), base64: "CQgH" });
    const session = createAudioCaptureSession({
      recorder,
      requestPermissions: jest.fn(async () => ({ granted: true })),
      fileFromUri: jest.fn(() => file),
    });

    await session.start();
    await expect(session.stopAndRead()).resolves.toEqual({
      status: "ready",
      uri: "file:///tmp/audio.m4a",
      audioBase64: "CQgH",
      rawBytes: 3,
      durationMs: 1234,
      metering: -14,
    });
    expect(recorder.stop).toHaveBeenCalled();
    expect(file.bytes).toHaveBeenCalled();
    expect(file.base64).toHaveBeenCalled();
    expect(file.delete).toHaveBeenCalled();
  });

  it("rejects oversized clips and deletes temp file", async () => {
    const recorder = makeRecorder();
    const file = makeFile({
      bytes: new Uint8Array([1]),
      base64: "a".repeat(MAX_AUDIO_BASE64_CHARS + 1),
    });
    const session = createAudioCaptureSession({
      recorder,
      requestPermissions: jest.fn(async () => ({ granted: true })),
      fileFromUri: jest.fn(() => file),
    });

    await session.start();
    await expect(session.stopAndRead()).resolves.toEqual({
      status: "clip-too-long",
    });
    expect(file.delete).toHaveBeenCalled();
  });
});
