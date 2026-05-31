import { createMemo, createSignal, For, Show } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { store, setStore } from '../store';

export default function RagLabTab() {
  const [showRaw, setShowRaw] = createSignal(false);
  const [draftPattern, setDraftPattern] = createSignal('');
  const [patternError, setPatternError] = createSignal<string | null>(null);
  const [indexedFileTypes, setIndexedFileTypes] = createSignal<string[]>([]);
  const [suggestionsOpen, setSuggestionsOpen] = createSignal(false);
  const [activeSuggestionIndex, setActiveSuggestionIndex] = createSignal(0);
  const [suppressBlurCommit, setSuppressBlurCommit] = createSignal(false);
  const [copied, setCopied] = createSignal(false);

  const availableSuggestions = createMemo(() => {
    const draft = draftPattern().trim().toLowerCase();
    const selected = new Set(store.ragLab.fileTypeFilters.map((pattern) => pattern.toLowerCase()));

    return indexedFileTypes()
      .map((ext) => `*.${ext}`)
      .filter((pattern) => !selected.has(pattern.toLowerCase()))
      .filter((pattern) => {
        if (!draft) return true;
        const ext = pattern.slice(2);
        return pattern.toLowerCase().startsWith(draft) || ext.startsWith(draft);
      });
  });

  const topKPresets = [1, 3, 5, 10, 15, 20];

  function closeSuggestions() {
    setSuggestionsOpen(false);
    setActiveSuggestionIndex(0);
  }

  async function loadSuggestions() {
    try {
      const fileTypes = await invoke<string[]>('list_indexed_file_types');
      setIndexedFileTypes(fileTypes);
      if (fileTypes.length > 0) {
        setSuggestionsOpen(true);
        setActiveSuggestionIndex(0);
      }
    } catch {
      setIndexedFileTypes([]);
      closeSuggestions();
    }
  }

  function resetParams() {
    setStore('ragLab', 'topK', 5);
    setStore('ragLab', 'fileTypeFilters', []);
    setDraftPattern('');
    setPatternError(null);
    closeSuggestions();
  }

  async function addChip(explicitCandidate?: string) {
    const candidate = (explicitCandidate ?? draftPattern()).trim();
    if (!candidate) return;
    if (store.ragLab.fileTypeFilters.includes(candidate)) {
      setPatternError('Pattern already added');
      closeSuggestions();
      return;
    }
    try {
      await invoke('validate_file_filter', { patterns: [candidate] });
      setStore('ragLab', 'fileTypeFilters', [
        ...store.ragLab.fileTypeFilters,
        candidate,
      ]);
      setDraftPattern('');
      setPatternError(null);
      closeSuggestions();
    } catch (err) {
      setPatternError(String(err));
    }
  }

  function removeChip(pattern: string) {
    setStore(
      'ragLab',
      'fileTypeFilters',
      store.ragLab.fileTypeFilters.filter((p) => p !== pattern)
    );
    setPatternError(null);
  }

  function selectSuggestion(index: number) {
    const suggestion = availableSuggestions()[index];
    if (!suggestion) return;
    void addChip(suggestion);
  }

  function onPatternKeyDown(e: KeyboardEvent) {
    const suggestions = availableSuggestions();
    if (e.key === 'ArrowDown' && suggestions.length > 0) {
      e.preventDefault();
      setSuggestionsOpen(true);
      setActiveSuggestionIndex((idx) => (idx + 1) % suggestions.length);
    } else if (e.key === 'ArrowUp' && suggestions.length > 0) {
      e.preventDefault();
      setSuggestionsOpen(true);
      setActiveSuggestionIndex((idx) => (idx - 1 + suggestions.length) % suggestions.length);
    } else if (e.key === 'Escape') {
      closeSuggestions();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (suggestionsOpen() && suggestions.length > 0) {
        selectSuggestion(activeSuggestionIndex());
      } else {
        void addChip();
      }
    } else if (e.key === 'Backspace' && draftPattern() === '') {
      const last = store.ragLab.fileTypeFilters[store.ragLab.fileTypeFilters.length - 1];
      if (last) removeChip(last);
    }
  }

  async function copyRawJson() {
    try {
      await navigator.clipboard.writeText(JSON.stringify(store.lastRawResponse, null, 2));
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Fallback: select the text
      const pre = document.querySelector('.raw-json') as HTMLPreElement | null;
      if (pre) {
        const range = document.createRange();
        range.selectNodeContents(pre);
        const sel = window.getSelection();
        sel?.removeAllRanges();
        sel?.addRange(range);
      }
    }
  }

  return (
    <div class="tab-content rag-lab-tab">
      <h2>RAG Lab</h2>

      <div class="rag-section">
        <h3>Parameters</h3>
        <label class="field field-block">
          <span>Top K ({store.ragLab.topK})</span>
          <div class="slider-container">
            <div class="slider-ticks">
              <For each={topKPresets}>
                {(val) => (
                  <button
                    class="slider-tick"
                    classList={{ active: store.ragLab.topK === val }}
                    onClick={() => setStore('ragLab', 'topK', val)}
                  >
                    {val}
                  </button>
                )}
              </For>
            </div>
            <input
              type="range"
              class="styled-slider"
              min={1}
              max={20}
              value={store.ragLab.topK}
              onInput={(e) => setStore('ragLab', 'topK', parseInt(e.currentTarget.value, 10))}
            />
          </div>
        </label>
        <label class="field glob-field">
          <span>File Filters (glob)</span>
          <div class="glob-input-shell">
            <div class="glob-chip-input" classList={{ 'has-error': patternError() !== null }}>
              <For each={store.ragLab.fileTypeFilters}>
                {(pattern) => (
                  <span class="glob-chip">
                    <span class="glob-chip-text">{pattern}</span>
                    <button
                      class="glob-chip-remove"
                      title="Remove"
                      onClick={() => removeChip(pattern)}
                    >
                      ×
                    </button>
                  </span>
                )}
              </For>
              <input
                type="text"
                class="glob-input"
                placeholder={
                  store.ragLab.fileTypeFilters.length === 0
                    ? 'e.g. *.rs, **/*.md, src/**/*.{ts,tsx}'
                    : ''
                }
                value={draftPattern()}
                onFocus={() => void loadSuggestions()}
                onInput={(e) => {
                  setDraftPattern(e.currentTarget.value);
                  setPatternError(null);
                  if (indexedFileTypes().length > 0) {
                    setSuggestionsOpen(true);
                    setActiveSuggestionIndex(0);
                  }
                }}
                onKeyDown={onPatternKeyDown}
                onBlur={() => {
                  window.setTimeout(() => {
                    if (!suppressBlurCommit()) {
                      void addChip();
                    }
                    setSuppressBlurCommit(false);
                    closeSuggestions();
                  }, 100);
                }}
              />
            </div>
            <Show when={suggestionsOpen() && availableSuggestions().length > 0}>
              <div class="glob-suggestion-list">
                <For each={availableSuggestions()}>
                  {(suggestion, index) => (
                    <button
                      class="glob-suggestion-item"
                      classList={{ active: index() === activeSuggestionIndex() }}
                      onMouseDown={(e) => {
                        e.preventDefault();
                        setSuppressBlurCommit(true);
                      }}
                      onClick={() => selectSuggestion(index())}
                    >
                      {suggestion}
                    </button>
                  )}
                </For>
              </div>
            </Show>
          </div>
          <Show when={patternError()}>
            <span class="field-error">{patternError()}</span>
          </Show>
        </label>
        <button class="action-btn reset-btn" onClick={resetParams}>
          Reset
        </button>
      </div>

      <div class="rag-section">
        <h3>Debug Telemetry</h3>
        <div class="telemetry-grid">
          <div class="telemetry-item">
            <span class="telemetry-label">Latency</span>
            <span class="telemetry-value">
              {store.lastSearchLatencyMs !== null ? `${store.lastSearchLatencyMs} ms` : '—'}
            </span>
          </div>
          <div class="telemetry-item">
            <span class="telemetry-label">Results</span>
            <span class="telemetry-value">{store.results.length}</span>
          </div>
          <div class="telemetry-item">
            <span class="telemetry-label">Embedder</span>
            <span class="telemetry-value">{store.indexingStatus.active_embedder || store.config.ollama_model}</span>
          </div>
        </div>
      </div>

      <div class="rag-section">
        <h3>
          <button class="collapsible-toggle" onClick={() => setShowRaw((v) => !v)}>
            Raw JSON {showRaw() ? '▾' : '▸'}
          </button>
        </h3>
        <Show when={showRaw()}>
          <div class="raw-json-wrapper">
            <pre class="raw-json">{JSON.stringify(store.lastRawResponse, null, 2)}</pre>
            <button class="copy-json-btn" onClick={copyRawJson}>
              {copied() ? 'Copied!' : 'Copy'}
            </button>
          </div>
        </Show>
      </div>
    </div>
  );
}
