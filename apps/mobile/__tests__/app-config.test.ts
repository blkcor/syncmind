import appConfig from "../app.json";

const microphonePermission =
  "SyncMind uses the microphone to record audio captures you send to your paired desktop.";

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
});
