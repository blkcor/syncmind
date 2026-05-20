import { createStore } from 'solid-js/store';
import type { SearchResult, Config, IndexingStatus } from '@syncmind/types';

export interface RagLabState {
  topK: number;
  fileTypeFilters: string[];
}

export interface AppState {
  query: string;
  results: SearchResult[];
  selectedIndex: number;
  loading: boolean;
  config: Config;
  indexingStatus: IndexingStatus;
  ragLab: RagLabState;
  activeTab: 'search' | 'rag-lab' | 'settings';
  copiedToast: boolean;
  lastSearchLatencyMs: number | null;
  lastRawResponse: unknown | null;
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
};

const defaultIndexingStatus: IndexingStatus = {
  file_count: 0,
  chunk_count: 0,
  last_updated: null,
  recent_errors: [],
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
});
