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

US-040 device identity uses the local native module `SyncMindDeviceIdentity`.
Expo Go does not include local native modules, so identity creation, signing,
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
- Scan the QR code with Expo Go only for screens that do not depend on local native modules

## Development Scripts

```bash
pnpm --filter mobile typecheck   # TypeScript type checking
pnpm --filter mobile lint        # ESLint
pnpm --filter mobile start       # Start Expo dev server
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
