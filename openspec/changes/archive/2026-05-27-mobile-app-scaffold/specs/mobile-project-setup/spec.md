# mobile-project-setup Specification

## Purpose
Initialize the Expo SDK 53 TypeScript project at `apps/mobile/` with full monorepo integration (pnpm workspace, shared `@syncmind/types`, `@syncmind/eslint-config`, `@syncmind/ts-config`, Zustand store pattern, and EAS Build profiles). This is the foundational scaffold for Phase 4 Mobile Capture.

## Requirements

### Requirement: Expo project initialized at apps/mobile/
The system SHALL provide an Expo SDK 53 TypeScript project rooted at `apps/mobile/` targeting iOS and Android platforms.

#### Scenario: Default Expo project structure
- **WHEN** a developer runs `ls apps/mobile/`
- **THEN** the directory contains `app.json`, `tsconfig.json`, `package.json`, `App.tsx`, and supporting Expo scaffold files
- **AND** `app.json` declares `expo.sdkVersion` compatible with SDK 53

#### Scenario: Expo CLI version check
- **WHEN** a developer runs `npx expo --version`
- **THEN** the version reported is compatible with SDK 53 (≥ 53.x)

### Requirement: pnpm workspace registration
The system SHALL register `apps/mobile/` in the root pnpm workspace and resolve `@syncmind/*` workspace dependencies.

#### Scenario: Workspace member listed
- **WHEN** the developer runs `pnpm ls --recursive --depth 0`
- **THEN** `mobile` appears in the package list

#### Scenario: Shared packages resolved
- **WHEN** the developer runs `pnpm install`
- **THEN** `@syncmind/types`, `@syncmind/ui-kit`, `@syncmind/eslint-config`, and `@syncmind/ts-config` are resolved from the pnpm workspace (not from a registry)
- **AND** `pnpm ls @syncmind/types` shows the correct local path

### Requirement: TypeScript configuration
The system SHALL provide a TypeScript configuration that extends `@syncmind/ts-config/base.json`.

#### Scenario: tsconfig extends shared base
- **WHEN** a developer inspects `apps/mobile/tsconfig.json`
- **THEN** it contains `"extends": "@syncmind/ts-config/base.json"`
- **AND** it declares a `@/*` path alias mapping to `./src/*`

#### Scenario: TypeScript type-checks pass
- **WHEN** a developer runs `pnpm --filter mobile typecheck`
- **THEN** the command completes with zero errors and zero warnings

### Requirement: ESLint configuration
The system SHALL provide an ESLint configuration that extends `@syncmind/eslint-config`.

#### Scenario: ESLint passes on scaffold code
- **WHEN** a developer runs `pnpm --filter mobile lint`
- **THEN** the command completes with zero errors (warnings from template boilerplate are acceptable)

### Requirement: Zustand store pattern
The system SHALL provide a minimal Zustand store stub at `apps/mobile/src/store.ts` consistent with the desktop app's store pattern.

#### Scenario: Store stub exists and type-checks
- **WHEN** a developer opens `apps/mobile/src/store.ts`
- **THEN** the file exports a `useAppStore` hook via Zustand `create()` with a typed `AppState` interface
- **AND** it type-checks without errors in `pnpm --filter mobile typecheck`

### Requirement: EAS Build profiles
The system SHALL declare EAS Build profiles for `development`, `preview`, and `production` in `eas.json`.

#### Scenario: eas.json has all three profiles
- **WHEN** a developer reads `apps/mobile/eas.json`
- **THEN** it defines `build.development`, `build.preview`, and `build.production` profiles
- **AND** the `development` profile sets `"developmentClient": true`
- **AND** the `production` profile sets `"distribution": "internal"` (MVP scope)

#### Scenario: EAS profiles pass validation
- **WHEN** a developer runs `npx eas build:configure` (or `eas build --profile development --dry-run`)
- **THEN** the command validates the profiles without errors

### Requirement: Development README
The system SHALL document local development commands in `apps/mobile/README.md`.

#### Scenario: README contains start command
- **WHEN** a developer reads `apps/mobile/README.md`
- **THEN** it includes the command `pnpm --filter mobile start` to start the Expo dev server
- **AND** it documents how to open the app on a device via Expo Go
