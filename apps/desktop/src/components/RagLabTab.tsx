import { createSignal, Show, For } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { store, setStore } from '../store';

export default function RagLabTab() {
  const [showRaw, setShowRaw] = createSignal(false);
  const [draftPattern, setDraftPattern] = createSignal('');
  const [patternError, setPatternError] = createSignal<string | null>(null);

  function resetParams() {
    setStore('ragLab', 'topK', 5);
    setStore('ragLab', 'fileTypeFilters', []);
    setDraftPattern('');
    setPatternError(null);
  }

  async function addChip() {
    const candidate = draftPattern().trim();
    if (!candidate) return;
    if (store.ragLab.fileTypeFilters.includes(candidate)) {
      setPatternError('Pattern already added');
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

  function onPatternKeyDown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      addChip();
    } else if (e.key === 'Backspace' && draftPattern() === '') {
      const last = store.ragLab.fileTypeFilters[store.ragLab.fileTypeFilters.length - 1];
      if (last) removeChip(last);
    }
  }

  return (
    <div class="tab-content rag-lab-tab">
      <h2>RAG Lab</h2>

      <div class="rag-section">
        <h3>Parameters</h3>
        <label class="field">
          <span>Top K ({store.ragLab.topK})</span>
          <input
            type="range"
            min={1}
            max={20}
            value={store.ragLab.topK}
            onInput={(e) => setStore('ragLab', 'topK', parseInt(e.currentTarget.value, 10))}
          />
        </label>
        <label class="field glob-field">
          <span>File Filters (glob)</span>
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
              onInput={(e) => {
                setDraftPattern(e.currentTarget.value);
                setPatternError(null);
              }}
              onKeyDown={onPatternKeyDown}
              onBlur={() => addChip()}
            />
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
            <span class="telemetry-label">Model</span>
            <span class="telemetry-value">{store.config.ollama_model}</span>
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
          <pre class="raw-json">{JSON.stringify(store.lastRawResponse, null, 2)}</pre>
        </Show>
      </div>
    </div>
  );
}
