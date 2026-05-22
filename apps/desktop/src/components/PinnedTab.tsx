import { createSignal, createEffect, onMount, onCleanup, Show, For } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { store, setStore } from '../store';
import { togglePin, refreshPinnedList } from '../pins';
import type { SearchResult } from '@syncmind/types';

function truncatePath(path: string, max = 50): string {
  if (path.length <= max) return path;
  return '…' + path.slice(-(max - 1));
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

function copyContent(content: string) {
  navigator.clipboard.writeText(content).then(() => {
    setStore('copiedToast', true);
    setTimeout(() => setStore('copiedToast', false), 2000);
  });
}

export default function PinnedTab() {
  const [selectedIndex, setSelectedIndex] = createSignal(0);

  onMount(() => {
    refreshPinnedList();
    window.addEventListener('keydown', onKeyDown);
  });

  onCleanup(() => {
    window.removeEventListener('keydown', onKeyDown);
  });

  createEffect(() => {
    const len = store.pinnedList.length;
    if (selectedIndex() >= len) {
      setSelectedIndex(Math.max(0, len - 1));
    }
  });

  function onKeyDown(e: KeyboardEvent) {
    if (store.activeTab !== 'pinned' || store.pinnedList.length === 0) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSelectedIndex((i) => Math.min(i + 1, store.pinnedList.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelectedIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const item = store.pinnedList[selectedIndex()];
      if (!item) return;
      if (e.metaKey || e.ctrlKey) {
        invoke('open_file', { path: item.file_path }).catch(console.error);
      } else {
        copyContent(item.content);
      }
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'p' && !e.shiftKey) {
      e.preventDefault();
      const item = store.pinnedList[selectedIndex()];
      if (item) togglePin(item.chunk_id);
    }
  }

  return (
    <div class="search-tab pinned-tab">
      <Show
        when={store.pinnedList.length > 0}
        fallback={
          <div class="empty-state">
            No pinned items yet. Press Cmd+P on a search result to pin it.
          </div>
        }
      >
        <div class="results-panel">
          <For each={store.pinnedList}>
            {(item: SearchResult, index) => (
              <div
                class="result-item"
                classList={{ selected: index() === selectedIndex() }}
                onClick={() => setSelectedIndex(index())}
              >
                <div class="result-meta">
                  <span class="result-icon">{fileIcon(item.file_path)}</span>
                  <span class="result-path" title={item.file_path}>
                    {truncatePath(item.file_path)}
                  </span>
                  <button
                    class="pin-toggle pinned"
                    title="Unpin (Cmd+P)"
                    onClick={(e) => {
                      e.stopPropagation();
                      togglePin(item.chunk_id);
                    }}
                  >
                    ★
                  </button>
                </div>
                <div class="result-preview">
                  {item.content.slice(0, 120).replace(/\s+/g, ' ')}
                </div>
              </div>
            )}
          </For>
        </div>
      </Show>

      <Show when={store.copiedToast}>
        <div class="toast">Copied!</div>
      </Show>
    </div>
  );
}
