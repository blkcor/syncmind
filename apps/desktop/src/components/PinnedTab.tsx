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
  const [editingTagsFor, setEditingTagsFor] = createSignal<number | null>(null);
  const [tagDraft, setTagDraft] = createSignal('');
  const [tagFilter, setTagFilter] = createSignal('');
  let tagInputRef: HTMLInputElement | undefined;

  const filteredList = () => {
    const filter = tagFilter().trim().toLowerCase();
    if (!filter) return store.pinnedList;
    return store.pinnedList.filter((item) =>
      item.tags?.some((t) => t.toLowerCase().includes(filter))
    );
  };

  const allTags = () => {
    const tags = new Set<string>();
    store.pinnedList.forEach((item) => (item.tags || []).forEach((t) => tags.add(t)));
    return [...tags].sort();
  };

  onMount(() => {
    refreshPinnedList();
    window.addEventListener('keydown', onKeyDown);
  });

  onCleanup(() => {
    window.removeEventListener('keydown', onKeyDown);
  });

  createEffect(() => {
    const len = filteredList().length;
    if (selectedIndex() >= len) {
      setSelectedIndex(Math.max(0, len - 1));
    }
  });

  async function addTag(chunkId: number, tag: string) {
    const item = store.pinnedList.find((p) => p.chunk_id === chunkId);
    if (!item) return;
    const current = item.tags || [];
    if (current.includes(tag)) return;
    const updated = [...current, tag];
    try {
      await invoke('update_pin_tags', { chunkId, tags: updated });
      setStore(
        'pinnedList',
        (p) => p.chunk_id === chunkId,
        'tags',
        updated
      );
    } catch (e) {
      console.error('Failed to add tag', e);
    }
  }

  async function removeTag(chunkId: number, tag: string) {
    const item = store.pinnedList.find((p) => p.chunk_id === chunkId);
    if (!item) return;
    const updated = (item.tags || []).filter((t) => t !== tag);
    try {
      await invoke('update_pin_tags', { chunkId, tags: updated });
      setStore(
        'pinnedList',
        (p) => p.chunk_id === chunkId,
        'tags',
        updated
      );
    } catch (e) {
      console.error('Failed to remove tag', e);
    }
  }

  function startEditingTags(chunkId: number) {
    setEditingTagsFor(chunkId);
    setTagDraft('');
    setTimeout(() => tagInputRef?.focus(), 50);
  }

  function onTagInputKeyDown(e: KeyboardEvent, chunkId: number) {
    if (e.key === 'Enter') {
      e.preventDefault();
      const tag = tagDraft().trim();
      if (tag) addTag(chunkId, tag);
      setTagDraft('');
    } else if (e.key === 'Escape') {
      setEditingTagsFor(null);
      setTagDraft('');
    }
  }

  function onKeyDown(e: KeyboardEvent) {
    if (store.activeTab !== 'pinned') return;
    // Don't intercept when editing tags
    if (editingTagsFor() !== null) return;

    if (filteredList().length === 0) return;

    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSelectedIndex((i) => Math.min(i + 1, filteredList().length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelectedIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const item = filteredList()[selectedIndex()];
      if (!item) return;
      if (e.metaKey || e.ctrlKey) {
        invoke('open_file', { path: item.file_path }).catch(console.error);
      } else {
        copyContent(item.content);
      }
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'p' && !e.shiftKey) {
      e.preventDefault();
      const item = filteredList()[selectedIndex()];
      if (item) togglePin(item.chunk_id);
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 't') {
      e.preventDefault();
      const item = filteredList()[selectedIndex()];
      if (item) startEditingTags(item.chunk_id);
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
        <Show when={allTags().length > 0}>
          <div class="pin-tag-filter">
            <span class="pin-tag-filter-label">Filter:</span>
            <div class="pin-tag-filter-chips">
              <button
                class="pin-filter-chip"
                classList={{ active: tagFilter() === '' }}
                onClick={() => setTagFilter('')}
              >
                All
              </button>
              <For each={allTags()}>
                {(tag) => (
                  <button
                    class="pin-filter-chip"
                    classList={{ active: tagFilter() === tag }}
                    onClick={() => setTagFilter(tagFilter() === tag ? '' : tag)}
                  >
                    {tag}
                  </button>
                )}
              </For>
            </div>
          </div>
        </Show>

        <div class="results-panel">
          <For each={filteredList()}>
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

                <div class="pin-tags-row">
                  <Show when={(item.tags || []).length > 0}>
                    <For each={item.tags || []}>
                      {(tag) => (
                        <span class="pin-tag">
                          <span class="pin-tag-text">{tag}</span>
                          <button
                            class="pin-tag-remove"
                            title={`Remove tag "${tag}"`}
                            onClick={(e) => {
                              e.stopPropagation();
                              removeTag(item.chunk_id, tag);
                            }}
                          >
                            ×
                          </button>
                        </span>
                      )}
                    </For>
                  </Show>
                  <Show when={editingTagsFor() === item.chunk_id}>
                    <div class="pin-tag-input-shell">
                      <input
                        ref={tagInputRef}
                        type="text"
                        class="pin-tag-input"
                        placeholder="tag name…"
                        value={tagDraft()}
                        onInput={(e) => setTagDraft(e.currentTarget.value)}
                        onKeyDown={(e) => onTagInputKeyDown(e, item.chunk_id)}
                        onBlur={() => {
                          setTimeout(() => {
                            setEditingTagsFor(null);
                            setTagDraft('');
                          }, 150);
                        }}
                        onClick={(e) => e.stopPropagation()}
                      />
                    </div>
                  </Show>
                  <button
                    class="pin-add-tag-btn"
                    title="Add tag (Cmd+T)"
                    onClick={(e) => {
                      e.stopPropagation();
                      startEditingTags(item.chunk_id);
                    }}
                  >
                    + tag
                  </button>
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
