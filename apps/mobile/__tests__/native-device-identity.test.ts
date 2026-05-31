describe("NativeDeviceIdentity module boundary", () => {
  beforeEach(() => {
    jest.resetModules();
  });

  it("fails closed without crashing module import when the native module is unavailable", async () => {
    jest.doMock("expo-modules-core", () => ({
      requireNativeModule: jest.fn(() => {
        throw new Error("Cannot find native module 'SyncMindDeviceIdentity'");
      }),
      requireOptionalNativeModule: jest.fn(() => null),
    }));

    let NativeDeviceIdentity:
      | typeof import("../src/crypto/native-device-identity").default
      | undefined;

    jest.isolateModules(() => {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      NativeDeviceIdentity = require("../src/crypto/native-device-identity").default;
    });

    await expect(NativeDeviceIdentity?.ensureIdentity()).rejects.toThrow(
      "SyncMindDeviceIdentity native module is unavailable",
    );
  });
});
