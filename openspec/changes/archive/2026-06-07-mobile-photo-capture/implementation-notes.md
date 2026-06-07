## SDK 56 Image API Verification

- `expo-image-picker@56.0.16` is installed for SDK 56 camera and library selection.
- `expo-image-picker` exposes `requestCameraPermissionsAsync()`, `requestMediaLibraryPermissionsAsync()`, `launchCameraAsync()`, and `launchImageLibraryAsync()`.
- Picker launch options support `mediaTypes`, `allowsMultipleSelection`, `quality`, `base64`, and `exif`.
- Picker assets expose `uri`, `width`, `height`, optional `mimeType`, optional `base64`, and optional `exif`.
- `expo-image-manipulator@56.0.17` is installed because picker compression does not enforce the 2048 px long-edge resize requirement.
- `expo-image-manipulator` exposes `manipulateAsync(uri, actions, saveOptions)` with resize actions and `SaveFormat.JPEG`, `compress`, and `base64` save options.
- `expo-file-system@56.0.7` is already installed and exposes `File.info()`, `File.base64()`, and `File.delete()` for processed temp files.

## EXIF Preservation Limit

The picker can request and return EXIF metadata with `exif: true`, but the SDK 56 image manipulator save options do not expose an EXIF-preservation option for JPEG re-encode. Photo capture continues with JPEG normalization and records this limitation rather than storing originals or adding native code.

## Native Module Drift Guardrails

- `expo-image-picker` and `expo-image-manipulator` require a rebuilt development client; Metro reload or Expo Go cannot add these native modules to an already installed app.
- `apps/mobile/scripts/verify-native-modules.mjs` verifies package dependencies, Expo config plugins, permission config, and iOS/Android autolinking for photo capture native modules.
- `pnpm --filter mobile native:verify` is CI-safe and checks tracked config plus autolinking.
- `pnpm --filter mobile native:verify:generated` also checks generated iOS/Android projects after a local prebuild.
- `apps/mobile/plugins/with-expo-image-picker-linker-fix.cjs` persists the iOS `ExpoImagePicker` provider linker workaround through Expo prebuild instead of relying on ignored generated Podfile edits. It wires the patch into CocoaPods hooks and the Xcode `[Expo] Configure project` build phase because that phase regenerates `ExpoModulesProvider.swift` during `expo run:ios`.
- The same plugin enforces the iOS camera, photo-library, and microphone permission strings in generated `Info.plist`.
- If runtime reports `Cannot find native module 'ExponentImagePicker'`, rebuild and reinstall the dev client with `pnpm --filter mobile rebuild:ios` or `pnpm --filter mobile rebuild:android`.
