import type { RecordingOptions, RecordingStartOptions } from "expo-audio";

export const MAX_RECORDING_MS = 60_000;
export const MAX_AUDIO_RAW_BYTES = 8 * 1024 * 1024;
export const MAX_AUDIO_BASE64_CHARS = 11 * 1024 * 1024;

export const AUDIO_RECORDING_OPTIONS: RecordingOptions = {
  extension: ".m4a",
  sampleRate: 16_000,
  numberOfChannels: 1,
  bitRate: 32_000,
  isMeteringEnabled: true,
  android: {
    extension: ".m4a",
    outputFormat: "mpeg4",
    audioEncoder: "aac",
    sampleRate: 16_000,
  },
  ios: {
    extension: ".m4a",
    sampleRate: 16_000,
    outputFormat: "aac ",
    audioQuality: 0x60,
  },
  web: {
    mimeType: "audio/mp4",
    bitsPerSecond: 32_000,
  },
};

export type AudioSizeValidation =
  | { ok: true }
  | { ok: false; reason: "clip-too-long" };

export interface AudioRecorderLike {
  uri: string | null;
  prepareToRecordAsync(options?: Partial<RecordingOptions>): Promise<void>;
  record(options?: RecordingStartOptions): void;
  stop(): Promise<void>;
  getStatus(): {
    durationMillis: number;
    metering?: number;
    url: string | null;
  };
}

export interface AudioFileLike {
  bytes(): Promise<Uint8Array>;
  base64(): Promise<string>;
  info(): { exists?: boolean; size?: number | null };
  delete(): void;
}

export interface AudioCaptureSessionDeps {
  recorder: AudioRecorderLike;
  requestPermissions: () => Promise<{ granted: boolean }>;
  fileFromUri: (uri: string) => AudioFileLike;
}

export type AudioCaptureStartResult =
  | { status: "recording"; durationMs: number; metering?: number }
  | { status: "permission-denied" };

export type AudioCaptureReadResult =
  | {
      status: "ready";
      uri: string;
      audioBase64: string;
      rawBytes: number;
      durationMs: number;
      metering?: number;
    }
  | { status: "clip-too-long" }
  | { status: "missing-uri" };

export function validateAudioSize(
  rawBytes: number,
  audioBase64: string,
): AudioSizeValidation {
  if (rawBytes > MAX_AUDIO_RAW_BYTES || audioBase64.length > MAX_AUDIO_BASE64_CHARS) {
    return { ok: false, reason: "clip-too-long" };
  }
  return { ok: true };
}

export function createAudioCaptureSession(deps: AudioCaptureSessionDeps) {
  const { recorder, requestPermissions, fileFromUri } = deps;

  async function start(): Promise<AudioCaptureStartResult> {
    const permission = await requestPermissions();
    if (!permission.granted) {
      return { status: "permission-denied" };
    }

    await recorder.prepareToRecordAsync();
    recorder.record({ forDuration: MAX_RECORDING_MS / 1000 });

    const status = recorder.getStatus();
    return {
      status: "recording",
      durationMs: status.durationMillis,
      metering: status.metering,
    };
  }

  async function stopAndRead(): Promise<AudioCaptureReadResult> {
    await recorder.stop();
    const status = recorder.getStatus();
    const uri = recorder.uri ?? status.url;
    if (!uri) {
      return { status: "missing-uri" };
    }

    const file = fileFromUri(uri);
    try {
      const info = file.info();
      if (typeof info.size === "number" && info.size > MAX_AUDIO_RAW_BYTES) {
        return { status: "clip-too-long" };
      }

      const bytes = await file.bytes();
      const audioBase64 = await file.base64();
      const rawBytes = typeof info.size === "number" ? info.size : bytes.byteLength;
      const validation = validateAudioSize(rawBytes, audioBase64);
      if (!validation.ok) {
        return { status: validation.reason };
      }

      return {
        status: "ready",
        uri,
        audioBase64,
        rawBytes,
        durationMs: status.durationMillis,
        metering: status.metering,
      };
    } finally {
      try {
        file.delete();
      } catch {
        // Best-effort cleanup; callers should not lose an encrypted enqueue result.
      }
    }
  }

  return { start, stopAndRead };
}
