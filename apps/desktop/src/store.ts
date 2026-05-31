import { createStore } from 'solid-js/store';
import type { SearchResult, Config, IndexingStatus, GlobPattern } from '@syncmind/types';

export interface RagLabState {
  topK: number;
  // Each entry is a single glob pattern (e.g. "*.rs", "**/*.md").
  // Empty list = no filter.
  fileTypeFilters: GlobPattern[];
}

export interface AppState {
  query: string;
  results: SearchResult[];
  selectedIndex: number;
  loading: boolean;
  config: Config;
  indexingStatus: IndexingStatus;
  ragLab: RagLabState;
  activeTab: 'search' | 'rag-lab' | 'settings' | 'pinned' | 'devices';
  copiedToast: boolean;
  lastSearchLatencyMs: number | null;
  lastRawResponse: unknown | null;
  // Set of chunk ids currently pinned. Updated optimistically on toggle,
  // reverted on backend failure.
  pinnedIds: Set<number>;
  // Cached list view for the Pinned tab; refreshed on tab open and after toggles.
  pinnedList: SearchResult[];
  // Reindex progress: null when idle, populated during rebuild.
  reindexProgress: { current: number; total: number; file_path: string } | null;
  // Pairing step progress: null when idle.
  pairingStep: string | null;
}

const defaultConfig: Config = {
  ollama_url: 'http://localhost:11434',
  ollama_model: 'bge-m3',
  mcp_transport: 'stdio',
  bind_addr: '127.0.0.1:3000',
  registered_files: [],
  embedding_dim: 1024,
  chunk_size: 512,
  chunk_overlap: 64,
  active_embedder: 'unknown',
  active_model: 'bge-m3',
  hybrid_search_enabled: true,
  reranker_enabled: false,
};

const defaultIndexingStatus: IndexingStatus = {
  file_count: 0,
  chunk_count: 0,
  last_updated: null,
  recent_errors: [],
  active_embedder: 'unknown',
  active_model: 'bge-m3',
};

export const [store, setStore] = createStore<AppState>({
  query: '',
  results: [],
  selectedIndex: 0,
  loading: false,
  config: defaultConfig,
  indexingStatus: defaultIndexingStatus,
  ragLab: {
    topK: 5,
    fileTypeFilters: [],
  },
  activeTab: 'search',
  copiedToast: false,
  lastSearchLatencyMs: null,
  lastRawResponse: null,
  pinnedIds: new Set<number>(),
  pinnedList: [],
  reindexProgress: null,
  pairingStep: null,
});
