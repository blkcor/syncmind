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

  it("keeps US-044 audio capture out of the text capture screen", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).not.toContain("Audio.Recording");
    expect(source).not.toContain("requestPermissionsAsync");
  });
});
