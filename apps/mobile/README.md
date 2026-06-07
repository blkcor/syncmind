# SyncMind Mobile

Mobile capture client for SyncMind — a lightweight Expo-based app for iOS and Android. Part of Phase 4 (Mobile Capture).

## Stack

- **Framework:** Expo SDK 56 (React Native 0.85)
- **Language:** TypeScript 6
- **State Management:** Zustand 5
- **Navigation:** Expo Router (file-based routing)

## Prerequisites

- Node.js 22+
- pnpm 11+
- [Expo Go](https://expo.dev/go) for JS-only screens
- Xcode (iOS simulator) and/or Android Studio (Android emulator) for native testing

US-040 device identity and US-045 photo capture use native modules, including
`SyncMindDeviceIdentity`, `expo-image-picker`, and `expo-image-manipulator`.
Expo Go does not include these modules, so capture, identity creation, signing,
biometric toggle, and reset must be tested in a development build or rebuilt
native app.

## Getting Started

```bash
# Install dependencies (from monorepo root)
pnpm install

# Start Expo dev server
pnpm --filter mobile start

# Or directly from apps/mobile/
cd apps/mobile && pnpm start
```

Once the dev server is running, press:
- `i` to open in iOS simulator
- `a` to open in Android emulator
- Scan the QR code with Expo Go only for screens that do not depend on native modules

## Native Development Builds

Rebuild and reinstall the native app after adding or changing any native
dependency, Expo config plugin, permission string, or local native module. Metro
reload only updates JavaScript; it cannot add native modules to an already
installed dev client.

```bash
pnpm --filter mobile native:verify
pnpm --filter mobile rebuild:ios
pnpm --filter mobile rebuild:android
pnpm --filter mobile native:verify:generated
```

If a simulator or device keeps launching an old app after a rebuild, uninstall
the stale build and reinstall:

```bash
xcrun simctl uninstall booted com.blkcor.syncmind
adb uninstall com.blkcor.syncmind
```

For an iPhone or iPad, delete the SyncMind app from the device first, then
install a fresh development build:

```bash
pnpm --dir apps/mobile exec expo prebuild --clean --platform ios
pnpm --dir apps/mobile exec expo start --dev-client --clear
pnpm --dir apps/mobile exec expo run:ios --device --no-bundler
```

If CocoaPods resolves `node` to a broken Homebrew install, switch to the project
Node version first, for example with `fnm use`, then rerun the same command.

`Cannot find native module 'ExponentImagePicker'` means the running native app
does not contain the image picker module. Do not debug that with JS reloads;
rebuild and reinstall the development build.

## Development Scripts

```bash
pnpm --filter mobile typecheck   # TypeScript type checking
pnpm --filter mobile lint        # ESLint
pnpm --filter mobile start       # Start Expo dev server
pnpm --filter mobile native:verify
pnpm --filter mobile native:verify:generated
```

## EAS Build

```bash
# Development build (includes Expo dev client)
eas build --profile development --platform ios

# Preview build (internal distribution)
eas build --profile preview --platform android

# Production build
eas build --profile production --platform all
```

## Project Structure

```
apps/mobile/
├── app/           # Expo Router pages (file-based routing)
├── components/    # Shared UI components
├── constants/     # App constants (theme colors, etc.)
├── assets/        # Images, fonts, etc.
├── src/           # App logic (store, crypto, services)
│   └── store.ts   # Zustand state store
├── app.json       # Expo configuration
├── eas.json       # EAS Build profiles
└── eslint.config.js
```

## Architecture

See `docs/prd/005-mobile-capture.md` for the full PRD.
