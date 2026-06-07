# Mobile Image Picker Native Module Drift

日期：2026-06-07

## 现象

移动端点击拍照或从相册选择时出现：

```text
Cannot find native module 'ExponentImagePicker'
```

## 排查结论

源码配置是正确的：

- `apps/mobile/package.json` includes `expo-image-picker@56.0.16`.
- `apps/mobile/package.json` includes `expo-image-manipulator@56.0.17`.
- `apps/mobile/app.json` declares the `expo-image-picker` config plugin and iOS camera/photo-library permission text.
- `expo-modules-autolinking resolve --platform ios --json` resolves `expo-image-picker` with `ImagePickerModule`.
- `expo-modules-autolinking resolve --platform android --json` resolves `expo-image-picker` with `expo.modules.imagepicker.ImagePickerModule`.

The runtime error means the installed native app was built before the native
module was included, or it was opened through Expo Go. Metro reload only updates
JavaScript and cannot add native modules to an existing binary.

## Durable Fix

The project now has a tracked native verification script:

```bash
pnpm --filter mobile native:verify
```

After regenerating native projects, verify generated native files too:

```bash
pnpm --filter mobile native:verify:generated
```

The iOS image picker linker workaround is tracked through
`apps/mobile/plugins/with-expo-image-picker-linker-fix.cjs` and registered in
`app.json`. This replaces the previous fragile state where the workaround lived
only in ignored `apps/mobile/ios/Podfile` output. The plugin wires the patch
into CocoaPods hooks and the Xcode `[Expo] Configure project` build phase
because that phase regenerates `ExpoModulesProvider.swift` during `expo run:ios`.
The same plugin also enforces the generated iOS camera, photo-library, and
microphone permission strings so `Info.plist` cannot drift from `app.json`
during prebuild.

## Runtime Fix

Stop the dev server, then rebuild and reinstall the native app:

```bash
pnpm --filter mobile rebuild:ios
pnpm --filter mobile rebuild:android
```

If the old app remains installed, uninstall it first:

```bash
xcrun simctl uninstall booted com.blkcor.syncmind
adb uninstall com.blkcor.syncmind
```

Then start Metro again:

```bash
pnpm --filter mobile start -- --clear
```

## Rule

Any change to Expo native dependencies, config plugins, permission strings, or
local native modules requires a native rebuild and reinstall. Expo Go is only for
JS-only screens in this app.
