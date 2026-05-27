## 1. Initialize Expo Project

- [x] 1.1 Run `npx create-expo-app@latest apps/mobile --template tabs --no-install` to scaffold Expo SDK 56 TypeScript project at `apps/mobile/`
- [x] 1.2 Verify scaffold structure: `apps/mobile/` contains `app.json`, `tsconfig.json`, `package.json`, `app/`, and supporting Expo files
- [x] 1.3 Verify `app.json` declares working Expo SDK configuration

## 2. pnpm Workspace Integration

- [x] 2.1 Root `pnpm-workspace.yaml` already covers `apps/mobile/*` via the existing `'apps/*'` glob
- [x] 2.2 Add `@syncmind/types`, `@syncmind/ui-kit`, `@syncmind/eslint-config`, `@syncmind/ts-config` as workspace dependencies in `apps/mobile/package.json`
- [x] 2.3 Add `typecheck` and `lint` scripts to `apps/mobile/package.json`: `"typecheck": "tsc --noEmit"` and `"lint": "eslint ."`
- [x] 2.4 Run `pnpm install` and verify workspace resolution: `pnpm ls --recursive --depth 0` shows `mobile` listed and workspace packages resolved from local paths

## 3. TypeScript Configuration

- [x] 3.1 Keep `expo/tsconfig.base` extend (shared `@syncmind/tsconfig` is SolidJS/DOM-specific and incompatible with React Native; `expo/tsconfig.base` is the correct base)
- [x] 3.2 `@/*` path alias already mapped via Expo template (pointing to `./*` which covers both root-level and `src/` imports)
- [x] 3.3 Create `apps/mobile/src/` directory structure placeholder
- [x] 3.4 Run `pnpm --filter mobile typecheck` and confirm zero errors and zero warnings

## 4. ESLint Configuration

- [x] 4.1 Create `apps/mobile/eslint.config.js` (flat config) using `typescript-eslint` + `eslint-plugin-react` + `eslint-plugin-react-hooks` (shared `@syncmind/eslint-config` is SolidJS-specific and incompatible with RN)
- [x] 4.2 Run `pnpm --filter mobile lint` and confirm zero errors

## 5. Zustand Store Pattern

- [x] 5.1 Add `zustand` as a dependency in `apps/mobile/package.json`
- [x] 5.2 Create `apps/mobile/src/store.ts` with typed `useAppStore` hook (Zustand v5 `create()` pattern) exporting `AppState` interface + actions
- [x] 5.3 Verify `pnpm --filter mobile typecheck` passes after store stub addition

## 6. EAS Build Configuration

- [x] 6.1 Create `apps/mobile/eas.json` with `development`, `preview`, and `production` profiles
- [x] 6.2 Set `development.developmentClient: true`
- [x] 6.3 Set `production.distribution: "internal"`
- [x] 6.4 Validate `eas.json` JSON syntax (EAS CLI not installed locally; JSON schema validated with `json.tool`)

## 7. README Documentation

- [x] 7.1 Create `apps/mobile/README.md` with local development commands (`pnpm --filter mobile start`), Expo Go usage, and EAS build instructions

## 8. End-to-End Verification

- [x] 8.1 Run `pnpm install` clean install from root
- [x] 8.2 Run `pnpm --filter mobile typecheck` — zero errors
- [x] 8.3 Run `pnpm --filter mobile lint` — zero errors
- [x] 8.4 Confirm `pnpm --filter mobile start` launches Expo dev server without crashes (Metro Bundler starts on port 8081)
