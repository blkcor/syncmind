## Context

PRD 005 (`docs/prd/005-mobile-capture.md`) defines Phase 4 of SyncMind: a lightweight Expo-based mobile capture client (`apps/mobile/`) for iOS and Android. The desktop-side protocol prerequisites (US-052, US-053, US-054) are complete and archived.

The monorepo today manages:
- Rust crates via Cargo workspace in `core/`
- Go service (Spine) in `services/sync-gateway/`
- Tauri + SolidJS desktop app in `apps/desktop/` (pnpm workspace member)
- Shared TypeScript packages in `packages/` (`types`, `ui-kit`, `eslint-config`, `ts-config`)

Before any Phase 4 feature work can start, the mobile app scaffold must exist as a first-class workspace member: Expo project initialized, shared configs wired in, TypeScript/Eslint toolchain passing, and EAS Build configured for CI/CD.

This change is the foundational "put the empty tent up" step — no feature code, no UI screens beyond the default Expo template.

## Goals / Non-Goals

**Goals:**
- Initialize `apps/mobile/` as an Expo SDK 53 TypeScript project targeting iOS + Android.
- Register `apps/mobile/` in the root pnpm workspace with `@syncmind/types`, `@syncmind/ui-kit`, `@syncmind/eslint-config`, `@syncmind/ts-config` as workspace dependencies.
- Configure `apps/mobile/tsconfig.json` extending `@syncmind/ts-config/base.json`.
- Add ESLint config extending `@syncmind/eslint-config`.
- Configure EAS Build (`eas.json`) with `development`, `preview`, and `production` profiles.
- Add `pnpm --filter mobile lint` and `pnpm --filter mobile typecheck` scripts.
- Create a minimal Zustand store stub (`apps/mobile/src/store.ts`) consistent with `apps/desktop/src/store.ts` patterns.
- Update root `pnpm-workspace.yaml` to include `apps/mobile/*`.
- Add README with local development commands.
- Verify the Expo dev server starts and renders the default template.

**Non-Goals:**
- No feature-specific UI, navigation, or screens beyond the default Expo template.
- No Zustand slices for feature state (pairing, captures, outbox — these come in their own changes).
- No native module configuration or prebuild (expo-dev-client setup deferred to when native modules are needed).
- No custom build tooling beyond Expo CLI + pnpm scripts.
- No Detox E2E test infrastructure.
- No internationalization (`i18n/`) scaffolding — deferred to US-043 (text capture).
- No CI pipeline configuration (EAS profiles are declared but not wired into GitHub Actions; CI integration deferred to a follow-up change).

## Decisions

### 1. Expo SDK 53 with default tabs template
**Rationale:** PRD 005 §US-039 specifies Expo SDK 53. The default TypeScript tabs template provides a starting point with tab navigation, TypeScript, and a clean project structure. The template's boilerplate will be trimmed to minimal scaffolding.

**Alternatives considered:** Blank template — rejected because the final app will need tab navigation (Capture, Recent, Search tabs per PRD 005); starting with tabs reduces restructuring later. Bare React Native workflow — rejected; Expo managed workflow is sufficient for MVP scope and avoids native build complexity.

### 2. Zustand for global state management
**Rationale:** PRD 005 §US-039 explicitly requires Zustand (matching `apps/desktop/src/store.ts`). Zustand is minimal, TypeScript-native, and works with Hermes engine.

**Alternatives considered:** Redux Toolkit — heavier for MVP. Jotai/Recoil — alternative but diverges from desktop convention.

### 3. ESLint flat config extending `@syncmind/eslint-config`
**Rationale:** The monorepo's shared ESLint config (`packages/eslint-config/`) provides consistent lint rules across all projects. Expo SDK 53 uses ESLint 9+ with flat config, which is compatible with the shared config's format.

**Alternatives considered:** Per-project ESLint config — rejected; diverges from monorepo convention.

### 4. TypeScript path aliases via `tsconfig.json` paths
**Rationale:** `@/` alias mapped to `./src/` mirrors the pattern in `apps/desktop/` and keeps imports clean as the source tree grows.

**Alternatives considered:** Relative imports only — leads to `../../../../` pain as depth increases.

### 5. EAS profiles without CI integration (MVP)
**Rationale:** PRD 005 §US-039 requires EAS profiles to exist. Declaring them in `eas.json` now means CI wiring later is a one-line `expo eas:build` call. MVP phase skips GitHub Actions integration because the mobile team (single developer) will build locally or via `eas build --profile development` on demand.

**Alternatives considered:** Full CI pipeline — premature for a scaffold change; deferred.

### 6. No Expo Router in scaffold
**Rationale:** Expo Router (file-based routing) is optional in SDK 53. The default tabs template includes `expo-router` by default, which is acceptable for MVP. If the developer later wants to switch to `react-navigation` directly, the change is isolated to the navigation layer.

**Decision:** Keep `expo-router` from the tabs template — it works, it's the recommended Expo navigation approach, and removing it is unnecessary churn at this stage.

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| **Expo SDK 53 may have Hermes compatibility edge cases** | Zustand, `@noble/curves` (US-040), and `expo-secure-store` are all verified Hermes-compatible. Initial scaffold avoids risky native modules. |
| **pnpm workspace resolution conflicts** | Pin workspace dependencies with exact versions in `apps/mobile/package.json`; verify resolution with `pnpm ls --depth 0`. |
| **EAS profile drift** | Profiles are declared but untested until the first EAS build. Document manual verification step in tasks.md. |
| **Expo Router lock-in** | Navigation is a thin abstraction; migrating to `react-navigation` later is a 1-day refactor affecting only the router config files. |
