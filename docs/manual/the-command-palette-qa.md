# 003 — The Command Palette: Manual QA Runbook

This runbook covers the manual verification tasks for `the-command-palette`
change. Sections are numbered to match `openspec/changes/the-command-palette/tasks.md`
so each tick in that file can cite a section here.

Run before declaring the change archive-ready.

## Prerequisites

```bash
# Clean state — start the dev build
cd ~/ai/project/syncmind
pnpm dev:desktop
```

Wait for the Vite dev server banner *and* the Tauri window to either appear
(first run) or stay hidden (subsequent runs). Confirm in the macOS menu bar
that the SyncMind tray icon is present.

Required test files — create or have on hand at least one of each:
- `~/desktop/qa/sample.rs` — any non-trivial Rust file
- `~/desktop/qa/sample.ts` — any non-trivial TypeScript file
- `~/desktop/qa/sample.md` — a Markdown file with headings
- `~/desktop/qa/sample.py` — a Python file with a function
- `~/desktop/qa/sample.go` — a Go file with a package
- `~/desktop/qa/will-fail.md` — a path you will *delete* to force an indexing error
  (used in the bonus tray test)

Register all of them via **Settings → Add File** before starting timed tests.

---

## 8.1 — Idle memory budget (< 150 MB combined)

**Procedure**

1. With `pnpm dev:desktop` running, open the palette once via
   `Cmd+Shift+Space` and then dismiss it.
2. Set a 5-minute timer. Do not interact with the app during this period.
3. Open Activity Monitor → search `syncmind`. You should see at least two
   processes:
   - `syncmind` (main Tauri process)
   - A WebKit helper (label varies: `WebKit.WebContent`, `Web Content`)
4. Record the **Memory** column for each. Sum them.

**Pass**

- Combined resident memory stays below **150 MB** for the full 5 minutes.

**Fail signatures**

- Combined > 150 MB at any sample.
- Memory climbs monotonically (suggests a leak in the indexing pipeline or
  embedding cache).
- Either process disappears (unexpected exit).

If memory climbs without a corresponding indexing event, capture
`~/.local/share/syncmind/logs/desktop.log` from the same window.

---

## 8.5 — Global hotkey responsiveness from external apps

**Procedure**

For each host application below, focus its window (click it, don't `Cmd+Tab`),
then press `Cmd+Shift+Space`:

| Host app | Hotkey behavior |
|---|---|
| VS Code | Palette appears, takes focus, search input has caret |
| Terminal / iTerm2 | Same |
| Browser (Chrome, Safari, Arc) | Same |
| Finder | Same |

After each show, press `Cmd+Shift+Space` again — the palette should hide.

**Pass**

- Palette appears within roughly 100 ms in every host (no perceptible delay).
- Caret lands in the search input automatically — no extra click needed.
- Toggle (show ↔ hide) works from the same hotkey.

**Fail signatures**

- Palette never appears (hotkey was eaten by the host app — known risk for
  VS Code if a user shortcut shadows it).
- Palette appears but does not gain focus (you can see it but typing goes to
  the host).
- Hotkey only works from some apps (suggests a permission issue with macOS
  System Settings → Privacy & Security → Accessibility / Input Monitoring).

If the hotkey is eaten by a host, file a follow-up rather than disabling the
check; the spec requires global availability.

---

## 8.6 — Hide-on-blur behavior

The most recent fix wires `WindowEvent::Focused(false) → window.hide()`.
This test confirms the wiring works across the three blur paths.

**Procedure**

For each scenario, start with the palette visible (`Cmd+Shift+Space`):

| Scenario | Action | Expected |
|---|---|---|
| Click outside | Click any visible region of another app | Palette hides immediately |
| App switch | Press `Cmd+Tab` and pick another app | Palette hides as soon as the other app activates |
| Space switch | Swipe to another Space with a 3-finger trackpad swipe (or `Ctrl+→`) | Palette hides; does not follow you to the new Space |
| `Esc` fallback | Press `Esc` while focused in the palette | Palette hides (this path predates the blur fix) |

**Pass**

- All four paths hide the palette within ~150 ms.
- The process stays alive (tray icon still visible in the menu bar).

**Fail signatures**

- Palette stays visible after blur — the `Focused(false)` handler did not
  fire. Check `desktop.log` for any panic in the event handler.
- Palette hides but the process exits (tray disappears). The lifetime
  contract is "hide, not close"; an exit here is a regression.
- Palette hides on blur but never reappears via `Cmd+Shift+Space` after
  (suggests the global shortcut handler crashed).

---

## 8.7 — Syntax highlighting for five languages

**Procedure**

1. From the Settings panel, confirm all five sample files are listed under
   **Registered Files**.
2. Wait for the indexing pipeline to finish (the **Indexing Dashboard** card
   should show a chunk count > 0 for each file and an updated timestamp).
3. Open the palette, search for a unique token from each file in turn, and
   click the first result so it appears in the preview pane.

| File extension | Expected highlighting (visual cues) |
|---|---|
| `.rs` | `fn`, `let`, `pub` in keyword color; lifetimes (`'a`) styled |
| `.ts` | `const`, `interface`, type annotations colored; generics in their own hue |
| `.md` | Headings bold; `#` markers visible; code fences boxed |
| `.py` | `def`, `class`, `import` colored; strings distinct from identifiers |
| `.go` | `func`, `package`, `import` colored; struct tags styled |

**Pass**

- All five files render with the github-dark theme's color palette (the
  shipped theme), not plain monospaced black text.
- Indentation is preserved (no collapsed whitespace).

**Fail signatures**

- File renders as plain text — Shiki's lazy loader failed to fetch the
  grammar. Check DevTools console (open via right-click → Inspect) for
  network or WASM errors.
- Wrong language applied (e.g. `.go` highlighted as TypeScript) — the
  extension-to-language map in `SearchTab.tsx` is wrong.
- App freezes during highlight — synchronous Shiki call on the main thread;
  the current code lazy-loads so this should not happen.

---

## 8.8 — Quick actions via keyboard and mouse

Each action must work from both an explicit click and the documented
keyboard shortcut.

**Procedure**

Open the palette, run any search that returns at least one result, and
select the first result.

| Action | Keyboard | Mouse | Expected |
|---|---|---|---|
| Copy chunk | `Enter` | Click **Copy** button in preview | Clipboard contains the chunk text; "Copied!" toast appears briefly |
| Open file | `Cmd+Enter` | Click **Open File** button | The system default app for that extension opens the file |
| Reveal in Finder | (no kbd binding) | Click **Reveal** button | Finder activates and highlights the file in its parent folder |

For Copy, paste into a separate text field (Notes app, terminal) and
verify the content matches what's shown in the preview.

**Pass**

- All five pairs (Copy kbd + Copy mouse, Open kbd + Open mouse, Reveal mouse)
  succeed.
- The toast appears for Copy in both paths.

**Fail signatures**

- Clipboard receives empty string or stale content — the `writeText` call
  failed or fired before the chunk was selected.
- "Open File" launches but the file is the wrong one (off-by-one indexing
  between selected result and Tauri command param).
- "Reveal" opens the parent folder but doesn't highlight the file (the
  macOS `open -R` flag missing).

---

## Bonus — Tray status indicator end-to-end (3.7 wiring)

Not in the formal task list, but worth confirming the new wiring while
you're at the keyboard.

**Procedure**

1. With the app running, go to Settings → Add File and register
   `~/qa/will-fail.md`.
2. Delete the file via Finder while the app is running.
3. Click outside the palette to dismiss, then look at the tray icon.
4. Save a new `~/qa/will-fail.md` (re-create the path).
5. Trigger a re-index via Settings → **Rebuild All**.

**Pass**

- After step 2 (or after the watcher's 1-second debounce notices the file
  is gone), the tray icon flips to the red-dot variant.
- After step 5, the tray flips back to the healthy template icon within a
  few seconds.
- `await invoke('get_indexing_status')` in DevTools shows `recent_errors`
  populating and clearing in sync with the icon.

**Fail signatures**

- Tray never flips — the `indexing-status-changed` event is not reaching
  the listener. Check `desktop.log` for `emit` errors.
- Tray flips to error but never recovers — `clear_error_for` is not being
  called on success, or the success path itself is failing silently.

---

## Reporting

For each section, record one of:

- ✅ **pass** — observed all "Pass" criteria; tick the corresponding
  `tasks.md` box.
- ❌ **fail** — note which fail signature matched and capture the relevant
  log lines from `~/.local/share/syncmind/logs/desktop.log`. File a
  follow-up issue or change rather than silently skipping.
- ⚠️ **partial** — some sub-scenarios passed but at least one did not.
  Treat as fail until resolved.

When all `8.x` boxes are green, the change is ready for
`openspec archive the-command-palette --yes`.
