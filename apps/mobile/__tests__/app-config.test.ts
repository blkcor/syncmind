import appConfig from "../app.json";

const microphonePermission =
  "SyncMind uses the microphone to record audio captures you send to your paired desktop.";
const cameraPermission =
  "SyncMind uses the camera to scan desktop pairing QR codes and take photo captures you send to your paired desktop.";
const imagePickerCameraPermission =
  "SyncMind uses the camera to take photo captures you send to your paired desktop.";
const photosPermission =
  "SyncMind uses your photo library to pick image captures you send to your paired desktop.";

describe("Expo app config", () => {
  it("declares microphone permissions for audio capture", () => {
    const audioPlugin = appConfig.expo.plugins.find((plugin) =>
      Array.isArray(plugin) && plugin[0] === "expo-audio"
    );

    expect(audioPlugin).toEqual([
      "expo-audio",
      expect.objectContaining({
        microphonePermission: expect.stringContaining("record audio captures"),
        recordAudioAndroid: true,
      }),
    ]);
  });

  it("declares the iOS microphone usage description for generated native builds", () => {
    expect(appConfig.expo.ios.infoPlist.NSMicrophoneUsageDescription).toBe(
      microphonePermission,
    );
  });

  it("declares camera permission copy for QR pairing and photo capture", () => {
    const cameraPlugin = appConfig.expo.plugins.find((plugin) =>
      Array.isArray(plugin) && plugin[0] === "expo-camera"
    );

    expect(cameraPlugin).toEqual([
      "expo-camera",
      expect.objectContaining({
        cameraPermission,
        microphonePermission,
        recordAudioAndroid: false,
      }),
    ]);
    expect(appConfig.expo.ios.infoPlist.NSCameraUsageDescription).toBe(
      cameraPermission,
    );
  });

  it("declares image picker camera and photo library permission copy", () => {
    const imagePickerPlugin = appConfig.expo.plugins.find((plugin) =>
      Array.isArray(plugin) && plugin[0] === "expo-image-picker"
    );

    expect(imagePickerPlugin).toEqual([
      "expo-image-picker",
      expect.objectContaining({
        cameraPermission: imagePickerCameraPermission,
        photosPermission,
        microphonePermission,
      }),
    ]);
    expect(appConfig.expo.ios.infoPlist.NSPhotoLibraryUsageDescription).toBe(
      photosPermission,
    );
  });

  it("declares the tracked image picker linker fix plugin", () => {
    expect(appConfig.expo.plugins).toContain(
      "./plugins/with-expo-image-picker-linker-fix.cjs",
    );
  });
});
