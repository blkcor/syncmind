## Why

Phase 4 (Mobile Capture) introduces the SyncMind mobile app — a lightweight Expo-based capture client for iOS and Android. Before any feature work can begin, the mobile application scaffold must be established inside the monorepo: Expo project initialization, pnpm workspace integration, shared TypeScript/Eslint config reuse, and EAS Build profiles for CI/CD.

This is the foundational change that unblocks all subsequent Phase 4 user stories (US-040 through US-051).

## What Changes

- Initialize `apps/mobile/` as an Expo SDK 53 project (TypeScript, iOS + Android targets).
- Register `apps/mobile/` in the root pnpm workspace, with dependencies on `@syncmind/types`, `@syncmind/ui-kit`, `@syncmind/eslint-config`, and `@syncmind/ts-config`.
- Configure `apps/mobile/tsconfig.json` extending `@syncmind/ts-config/base.json`.
- Add ESLint configuration extending `@syncmind/eslint-config`.
- Configure EAS Build profiles (`development` / `preview` / `production`) via `eas.json`.
- Add workspace-level scripts: `pnpm --filter mobile lint`, `pnpm --filter mobile typecheck`.
- Create a minimal Zustand-based store pattern (consistent with `apps/desktop/src/store.ts`) for future state management.
- Add README with local development instructions.
- **No** global state library beyond Zustand (MVP scope).
- **No** feature-specific UI or logic — this is pure scaffolding.

## Capabilities

### New Capabilities
- `mobile-project-setup`: Expo SDK 53 project scaffold in the monorepo, with build tooling and shared config integration.

### Modified Capabilities

None. This is a new application entry.

## Impact

- **New directory** `apps/mobile/` added to the pnpm workspace.
- **Root `pnpm-workspace.yaml`**: new `apps/mobile/*` glob entry.
- **`@syncmind/types`**, **`@syncmind/ui-kit`**, **`@syncmind/eslint-config`**, **`@syncmind/ts-config`**: become runtime build dependencies for `apps/mobile/`.
- **No changes** to `core/`, `services/`, `apps/desktop/`, or `packages/` source files.
