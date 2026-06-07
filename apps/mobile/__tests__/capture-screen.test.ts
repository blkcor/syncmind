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

  it("exposes US-045 photo capture from the paired capture toolbar only", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    const unpairedBranch = source.slice(
      source.indexOf("if (!isPaired)"),
      source.indexOf("return (", source.indexOf("if (!isPaired)") + 1),
    );

    expect(source).toContain("from 'expo-symbols'");
    expect(source).toContain('accessibilityLabel="Add photo"');
    expect(source).toContain("camera.fill");
    expect(unpairedBranch).not.toContain('accessibilityLabel="Add photo"');
  });

  it("does not load native image modules while evaluating the capture route", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).not.toContain("from 'expo-image-picker'");
    expect(source).not.toContain('from "expo-image-picker"');
    expect(source).not.toContain("from '@/src/capture/image'");
    expect(source).toContain("import('expo-image-picker')");
    expect(source).toContain("import('@/src/capture/image')");
  });

  it("wires the US-045 ActionSheet-style photo source choices", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("showPhotoSourcePicker");
    expect(source).toContain("ActionSheetIOS.showActionSheetWithOptions");
    expect(source).toContain("Take Photo");
    expect(source).toContain("Pick from Library");
    expect(source).toContain("Cancel");
  });

  it("requests camera and library permissions only inside their selected source handlers", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    const takePhotoHandler = source.slice(
      source.indexOf("const handleTakePhoto"),
      source.indexOf("const handlePickFromLibrary"),
    );
    const libraryHandler = source.slice(
      source.indexOf("const handlePickFromLibrary"),
      source.indexOf("const showPhotoSourcePicker"),
    );

    expect(takePhotoHandler).toContain("requestCameraPermissionsAsync");
    expect(takePhotoHandler).toContain("launchCameraAsync");
    expect(takePhotoHandler).not.toContain("requestMediaLibraryPermissionsAsync");
    expect(libraryHandler).toContain("requestMediaLibraryPermissionsAsync");
    expect(libraryHandler).toContain("launchImageLibraryAsync");
    expect(libraryHandler).not.toContain("requestCameraPermissionsAsync");
  });

  it("surfaces US-045 photo failure feedback without logging image plaintext", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("Enable camera access to take photos.");
    expect(source).toContain("Enable photo library access to pick images.");
    expect(source).toContain("Image too large - try a smaller photo.");
    expect(source).toContain("Could not select image.");
    expect(source).toContain("Could not prepare image.");
    expect(source).toContain("Capture queue is full - connect to upload or retry failed captures");
    expect(source).not.toContain("console.log(imageBase64");
    expect(source).not.toContain("console.error(imageBase64");
  });

  it("wires caption review and image enqueue through encrypted capture-image bundles", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    expect(source).toContain("photoCaptionModalVisible");
    expect(source).toContain("photoCaption.trim()");
    expect(source).toContain("handleSendImageCapture(null)");
    expect(source).toContain("handleCancelPhotoCaption");
    expect(source).toContain("preprocessSelectedImage");
    expect(source).toContain("validateCaptureImageSerializedSize");
    expect(source).toContain("createCaptureImagePayload");
    expect(source).toContain("encryptCaptureImage");
    expect(source).toContain("CAPTURE_IMAGE_CONTENT_TYPE");
    expect(source).toContain("Image capture");
    expect(source).not.toContain("/v1/media/upload");
  });

  it("keeps the photo caption modal usable while the keyboard is open", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    const captionModal = source.slice(
      source.indexOf('visible={photoCaptionModalVisible}'),
      source.indexOf('</Modal>', source.indexOf('visible={photoCaptionModalVisible}')),
    );

    expect(captionModal).toContain('style={styles.photoCaptionKeyboardAvoidingView}');
    expect(captionModal).toContain("behavior={Platform.OS === 'ios' ? 'padding' : 'height'}");
    expect(captionModal).toContain('testID="photo-caption-keyboard-dismiss-backdrop"');
    expect(captionModal).toContain("onPress={Keyboard.dismiss}");
    expect(captionModal).toContain('accessibilityLabel="Photo caption"');
    expect(captionModal).toContain('returnKeyType="done"');
    expect(captionModal).toContain("blurOnSubmit");
  });

  it("dismisses the keyboard before closing or sending the photo caption modal", () => {
    const source = fs.readFileSync(
      path.join(__dirname, "../app/(tabs)/index.tsx"),
      "utf8",
    );

    const cancelHandler = source.slice(
      source.indexOf("const handleCancelPhotoCaption"),
      source.indexOf("const handleSendImageCapture"),
    );
    const sendHandler = source.slice(
      source.indexOf("const handleSendImageCapture"),
      source.indexOf("const handleVoiceSwipeStart"),
    );

    expect(cancelHandler).toContain("Keyboard.dismiss()");
    expect(sendHandler).toContain("Keyboard.dismiss()");
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
