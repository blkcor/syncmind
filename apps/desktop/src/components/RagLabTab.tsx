import { createSignal, Show } from 'solid-js';
import { store, setStore } from '../store';

export default function RagLabTab() {
  const [showRaw, setShowRaw] = createSignal(false);

  function resetParams() {
    setStore('ragLab', 'topK', 5);
    setStore('ragLab', 'fileTypeFilters', []);
  }

  const filterText = () => store.ragLab.fileTypeFilters.join(',');

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
        <label class="field">
          <span>File Type Filters</span>
          <input
            type="text"
            placeholder="e.g. md,rs,ts"
            value={filterText()}
            onInput={(e) =>
              setStore(
                'ragLab',
                'fileTypeFilters',
                e.currentTarget.value
                  .split(',')
                  .map((s) => s.trim())
                  .filter(Boolean)
              )
            }
          />
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
