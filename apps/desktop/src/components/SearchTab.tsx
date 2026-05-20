import { createSignal, createEffect, onMount, onCleanup, Show, For } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { store, setStore } from '../store';
import type { SearchResult } from '@syncmind/types';
import {
  createHighlighter,
  type HighlighterGeneric,
  type BundledLanguage,
  type BundledTheme,
} from 'shiki';

let highlighterPromise: Promise<HighlighterGeneric<BundledLanguage, BundledTheme>> | null = null;
function getLazyHighlighter(): Promise<HighlighterGeneric<BundledLanguage, BundledTheme>> {
  if (!highlighterPromise) {
    highlighterPromise = createHighlighter({
      themes: ['github-dark'],
      langs: ['rust', 'markdown', 'python', 'typescript', 'go', 'json'],
    });
  }
  return highlighterPromise;
}

function extToLang(ext: string): string {
  switch (ext) {
    case 'rs':
      return 'rust';
    case 'md':
      return 'markdown';
    case 'py':
      return 'python';
    case 'ts':
    case 'tsx':
      return 'typescript';
    case 'go':
      return 'go';
    case 'json':
      return 'json';
    default:
      return 'text';
  }
}

function fileIcon(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  switch (ext) {
    case 'rs':
      return '⚙️';
    case 'md':
      return '📝';
    case 'py':
      return '🐍';
    case 'ts':
    case 'tsx':
      return '📘';
    case 'go':
      return '🐹';
    case 'pdf':
      return '📄';
    default:
      return '📄';
  }
}

function truncatePath(path: string, max = 50): string {
  if (path.length <= max) return path;
  return '…' + path.slice(-(max - 1));
}

function copyContent(content: string) {
  navigator.clipboard.writeText(content).then(() => {
    setStore('copiedToast', true);
    setTimeout(() => setStore('copiedToast', false), 2000);
  });
}

export default function SearchTab() {
  const [previewHtml, setPreviewHtml] = createSignal('');
  let searchTimer: number | null = null;
  let inputRef: HTMLInputElement | undefined;

  async function runSearch(query: string) {
    if (!query.trim()) {
      setStore('results', []);
      setStore('selectedIndex', 0);
      setStore('lastSearchLatencyMs', null);
      setStore('lastRawResponse', null);
      return;
    }
    setStore('loading', true);
    const start = Date.now();
    try {
      const results = await invoke<SearchResult[]>('search_knowledge', {
        query,
        topK: store.ragLab.topK,
      });
      setStore('lastSearchLatencyMs', Date.now() - start);
      setStore('lastRawResponse', results as unknown);
      setStore('results', results);
      setStore('selectedIndex', 0);
    } catch (e) {
      console.error('Search failed', e);
      setStore('results', []);
      setStore('lastSearchLatencyMs', Date.now() - start);
      setStore('lastRawResponse', { error: String(e) });
    } finally {
      setStore('loading', false);
    }
  }

  createEffect(() => {
    const q = store.query;
    if (searchTimer) window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => runSearch(q), 300);
  });

  createEffect(() => {
    const idx = store.selectedIndex;
    const result = store.results[idx];
    if (!result) {
      setPreviewHtml('');
      return;
    }
    const ext = result.file_path.split('.').pop()?.toLowerCase() ?? '';
    const lang = extToLang(ext);
    getLazyHighlighter().then((hl) => {
      const html = hl.codeToHtml(result.content, {
        lang,
        theme: 'github-dark',
      });
      setPreviewHtml(html);
    });
  });

  function onKeyDown(e: KeyboardEvent) {
    if (store.results.length === 0) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setStore('selectedIndex', (i) => Math.min(i + 1, store.results.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setStore('selectedIndex', (i) => Math.max(i - 1, 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const result = store.results[store.selectedIndex];
      if (!result) return;
      if (e.metaKey || e.ctrlKey) {
        invoke('open_file', { path: result.file_path }).catch(console.error);
      } else {
        copyContent(result.content);
      }
    }
  }

  onMount(() => {
    window.addEventListener('keydown', onKeyDown);
    inputRef?.focus();
  });

  onCleanup(() => {
    window.removeEventListener('keydown', onKeyDown);
    if (searchTimer) window.clearTimeout(searchTimer);
  });

  return (
    <div class="search-tab">
      <div class="search-header">
        <input
          ref={inputRef}
          class="search-input"
          type="text"
          placeholder="Search knowledge..."
          value={store.query}
          onInput={(e) => setStore('query', e.currentTarget.value)}
        />
        <Show when={store.loading}>
          <span class="spinner" />
        </Show>
      </div>

      <Show
        when={store.query.trim() !== '' || store.results.length > 0}
        fallback={
          <div class="empty-state">Start typing to search your knowledge...</div>
        }
      >
        <div class="search-body">
          <div class="results-panel">
            <Show
              when={store.results.length > 0}
              fallback={<div class="empty-state">No matches found. Try a broader query.</div>}
            >
              <For each={store.results}>
                {(result, index) => (
                  <div
                    class="result-item"
                    classList={{ selected: index() === store.selectedIndex }}
                    onClick={() => setStore('selectedIndex', index())}
                  >
                    <div class="result-meta">
                      <span class="result-icon">{fileIcon(result.file_path)}</span>
                      <span class="result-path" title={result.file_path}>
                        {truncatePath(result.file_path)}
                      </span>
                      <span class="result-score">{(result.score * 100).toFixed(1)}%</span>
                    </div>
                    <div class="result-preview">
                      {result.content.slice(0, 120).replace(/\s+/g, ' ')}
                    </div>
                  </div>
                )}
              </For>
            </Show>
          </div>

          <Show when={store.results.length > 0}>
            <div class="preview-panel">
              <div class="preview-header">
                <span class="preview-path" title={store.results[store.selectedIndex]?.file_path}>
                  {store.results[store.selectedIndex]?.file_path}
                </span>
                <span class="preview-lines">
                  {store.results[store.selectedIndex]?.start_line}-
                  {store.results[store.selectedIndex]?.end_line}
                </span>
              </div>
              <div class="preview-actions">
                <button
                  class="action-btn"
                  onClick={() => copyContent(store.results[store.selectedIndex]?.content ?? '')}
                >
                  Copy
                </button>
                <button
                  class="action-btn"
                  onClick={() =>
                    invoke('open_file', {
                      path: store.results[store.selectedIndex]?.file_path,
                    }).catch(console.error)
                  }
                >
                  Open File
                </button>
                <button
                  class="action-btn"
                  onClick={() =>
                    invoke('reveal_in_finder', {
                      path: store.results[store.selectedIndex]?.file_path,
                    }).catch(console.error)
                  }
                >
                  Reveal
                </button>
              </div>
              {/* Shiki output is sanitized HTML (escaped angle brackets, no user attrs) */}
              {/* eslint-disable-next-line solid/no-innerhtml */}
              <div class="preview-content" innerHTML={previewHtml()} />
            </div>
          </Show>
        </div>
      </Show>

      <Show when={store.copiedToast}>
        <div class="toast">Copied!</div>
      </Show>
    </div>
  );
}
