const PHOTO_NATIVE_MODULE_NAMES = [
  "ExponentImagePicker",
  "ExpoImageManipulator",
] as const;

export const PHOTO_NATIVE_MODULE_UNAVAILABLE_MESSAGE =
  "Rebuild and reinstall the mobile dev client to enable photo capture.";

export function isMissingPhotoNativeModuleError(error: unknown): boolean {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : "";

  return PHOTO_NATIVE_MODULE_NAMES.some((moduleName) =>
    message.includes(moduleName)
  );
}
