import fs from "node:fs";
import path from "node:path";

describe("Capture screen send behavior", () => {
  it("uses Expo Crypto.randomUUID for capture ids instead of browser global crypto", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("from 'expo-crypto'");
    expect(source).toContain("Crypto.randomUUID()");
    expect(source).not.toContain("crypto.randomUUID()");
  });

  it("renders the paired capture input as an autofocus multiline text box", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("autoFocus");
    expect(source).toContain("multiline");
    expect(source).toContain("textInputRef");
  });

  it("dismisses the keyboard when the capture surface is dragged", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain('keyboardDismissMode="on-drag"');
  });

  it("enforces local send eligibility and the US-043 text limit", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("MAX_CAPTURE_TEXT_CHARS = 50_000");
    expect(source).toContain("Too long - try splitting");
    expect(source).toContain("isTooLong");
    expect(source).toContain("canSend");
    expect(source).toContain("disabled={!canSend}");
  });

  it("maps connection status to the US-043 status row states", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("captureStatusLabel");
    expect(source).toContain("captureStatusColor");
    expect(source).toContain("Connected");
    expect(source).toContain("Queued locally");
    expect(source).toContain("Pairing invalid");
  });

  it("includes the local device fingerprint in capture-text payloads", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("ensureIdentity");
    expect(source).toContain("client_device_fingerprint");
    expect(source).not.toContain("source: \"mobile\"");
  });

  it("wires recent outbox status metadata into the paired capture screen", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("getRecentOutboxStatuses");
    expect(source).toContain("subscribeToOutboxChanges");
    expect(source).toContain("Recent sends");
    expect(source).toContain("outboxStatusRows");
    expect(source).not.toContain("encrypted_blob");
  });

  it("wires US-044 voice mode entry with swipe threshold and toggle button", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("VOICE_SWIPE_THRESHOLD_PX = 48");
    expect(source).toContain("handleVoiceSwipeStart");
    expect(source).toContain("handleVoiceSwipeEnd");
    expect(source).toContain('accessibilityLabel="Toggle capture mode"');
    expect(source).toContain("setCaptureMode((m) => (m === 'voice' ? 'text' : 'voice'))");
    expect(source).toContain("<VoiceRecorder onEnqueueClip={enqueueAudioClip} />");
  });

  it("wires press-and-hold plus accessibility action recording controls in VoiceRecorder", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../src/capture/VoiceRecorder.tsx"),
      "utf8",
    );

    expect(source).toContain("onPressIn={handleVoicePressIn}");
    expect(source).toContain("onPressOut={handleVoicePressOut}");
    expect(source).toContain("accessibilityActions");
    expect(source).toContain("onAccessibilityAction={handleVoiceAccessibilityAction}");
    expect(source).toContain("requestRecordingPermissionsAsync");
    expect(source).toContain("setAudioModeAsync");
  });

  it("statically imports expo-audio in VoiceRecorder for native module bundling", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../src/capture/VoiceRecorder.tsx"),
      "utf8",
    );

    expect(source).toContain("from 'expo-audio'");
    expect(source).not.toContain("import('expo-audio')");
  });

  it("encrypts and enqueues capture-audio bundles with audio content type", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("createCaptureAudioPayload");
    expect(source).toContain("encryptCaptureAudio");
    expect(source).toContain("CAPTURE_AUDIO_CONTENT_TYPE");
    expect(source).toContain("audioBase64");
    expect(source).toContain("durationMs");
    expect(source).toContain("Clip too long");
  });

  it("handles background interruption with keep and discard choices in VoiceRecorder", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../src/capture/VoiceRecorder.tsx"),
      "utf8",
    );

    expect(source).toContain("BACKGROUND_INTERRUPT_MS = 30_000");
    expect(source).toContain("AppState.addEventListener('change'");
    expect(source).toContain("handleInterruptedRecording");
    expect(source).toContain("keepInterruptedClip");
    expect(source).toContain("discardInterruptedClip");
    expect(source).toContain("setInterruptedClip");
  });
});
