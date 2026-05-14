import { createStore } from 'solid-js/store';
import { store, setStore } from './store';
import type { SearchResult } from '@syncmind/types';

function SearchTab() {
  return (
    <div class="tab-content">
      <div class="search-header">
        <input
          class="search-input"
          type="text"
          placeholder="Search knowledge..."
          value={store.query}
          onInput={(e) => setStore('query', e.currentTarget.value)}
          autofocus
        />
        {store.loading && <span class="spinner" />}
      </div>
      <div class="results-list">
        {store.results.length === 0 && !store.loading && store.query && (
          <div class="empty-state">No results found</div>
        )}
        {store.results.map((result: SearchResult, index: number) => (
          <div
            class="result-item"
            classList={{ selected: index === store.selectedIndex }}
            onClick={() => setStore('selectedIndex', index)}
          >
            <div class="result-meta">
              <span class="result-path">{result.file_path}</span>
              <span class="result-score">{(result.score * 100).toFixed(1)}%</span>
            </div>
            <pre class="result-content">{result.content}</pre>
          </div>
        ))}
      </div>
    </div>
  );
}

function RagLabTab() {
  return (
    <div class="tab-content">
      <h2>RAG Lab</h2>
      <label class="field">
        <span>Top K</span>
        <input
          type="number"
          min={1}
          max={50}
          value={store.ragLab.topK}
          onInput={(e) => setStore('ragLab', 'topK', parseInt(e.currentTarget.value, 10))}
        />
      </label>
      <label class="field">
        <span>File Type Filters</span>
        <input
          type="text"
          placeholder="e.g. md,rs,ts"
          value={store.ragLab.fileTypeFilters.join(',')}
          onInput={(e) =>
            setStore(
              'ragLab',
              'fileTypeFilters',
              e.currentTarget.value.split(',').map((s) => s.trim()).filter(Boolean)
            )
          }
        />
      </label>
    </div>
  );
}

function SettingsTab() {
  return (
    <div class="tab-content">
      <h2>Settings</h2>
      <div class="settings-section">
        <h3>Configuration</h3>
        <div class="field">
          <span>Ollama URL</span>
          <input type="text" value={store.config.ollama_url} readOnly />
        </div>
        <div class="field">
          <span>Model</span>
          <input type="text" value={store.config.ollama_model} readOnly />
        </div>
        <div class="field">
          <span>Transport</span>
          <input type="text" value={store.config.mcp_transport} readOnly />
        </div>
      </div>
      <div class="settings-section">
        <h3>Indexing Status</h3>
        <div class="field">
          <span>Files</span>
          <span>{store.indexingStatus.file_count}</span>
        </div>
        <div class="field">
          <span>Chunks</span>
          <span>{store.indexingStatus.chunk_count}</span>
        </div>
        <div class="field">
          <span>Last Updated</span>
          <span>{store.indexingStatus.last_updated ?? 'Never'}</span>
        </div>
      </div>
    </div>
  );
}

export default function App() {
  const tabs = [
    { key: 'search', label: 'Search' },
    { key: 'rag-lab', label: 'RAG Lab' },
    { key: 'settings', label: 'Settings' },
  ] as const;

  return (
    <div class="app">
      <nav class="tab-bar">
        {tabs.map((tab) => (
          <button
            class="tab-button"
            classList={{ active: store.activeTab === tab.key }}
            onClick={() => setStore('activeTab', tab.key)}
          >
            {tab.label}
          </button>
        ))}
      </nav>
      <main class="main">
        {store.activeTab === 'search' && <SearchTab />}
        {store.activeTab === 'rag-lab' && <RagLabTab />}
        {store.activeTab === 'settings' && <SettingsTab />}
      </main>
    </div>
  );
}
