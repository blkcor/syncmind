import {
  PHOTO_NATIVE_MODULE_UNAVAILABLE_MESSAGE,
  isMissingPhotoNativeModuleError,
} from "../src/capture/photoErrors";

describe("photo native module error handling", () => {
  it("detects missing Expo image picker native module errors", () => {
    expect(
      isMissingPhotoNativeModuleError(
        new Error("Cannot find native module 'ExponentImagePicker'"),
      ),
    ).toBe(true);
  });

  it("detects missing Expo image manipulator native module errors", () => {
    expect(
      isMissingPhotoNativeModuleError(
        new Error("Cannot find native module 'ExpoImageManipulator'"),
      ),
    ).toBe(true);
  });

  it("does not classify normal picker failures as native module drift", () => {
    expect(isMissingPhotoNativeModuleError(new Error("User cancelled picker"))).toBe(false);
  });

  it("uses an actionable rebuild message", () => {
    expect(PHOTO_NATIVE_MODULE_UNAVAILABLE_MESSAGE).toContain("Rebuild");
    expect(PHOTO_NATIVE_MODULE_UNAVAILABLE_MESSAGE).toContain("dev client");
  });
});
