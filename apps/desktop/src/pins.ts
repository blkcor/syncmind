import { invoke } from '@tauri-apps/api/core';
import type { SearchResult } from '@syncmind/types';
import { store, setStore } from './store';

/// Toggle a chunk's pin state with optimistic UI update.
/// Calls the backend and reverts the optimistic update on failure.
export async function togglePin(chunkId: number): Promise<void> {
  const wasPinned = store.pinnedIds.has(chunkId);
  const next = new Set(store.pinnedIds);
  if (wasPinned) {
    next.delete(chunkId);
  } else {
    next.add(chunkId);
  }
  setStore('pinnedIds', next);

  try {
    if (wasPinned) {
      await invoke('unpin_chunk', { chunkId });
    } else {
      await invoke('pin_chunk', { chunkId });
    }
    // Refresh cached pinned list so the Pinned tab updates without an extra fetch.
    await refreshPinnedList();
  } catch (err) {
    console.error('Pin toggle failed; reverting', err);
    const revert = new Set(store.pinnedIds);
    if (wasPinned) {
      revert.add(chunkId);
    } else {
      revert.delete(chunkId);
    }
    setStore('pinnedIds', revert);
  }
}

/// Fetch the current pinned list from the backend and cache it in the store.
export async function refreshPinnedList(): Promise<void> {
  try {
    const list = await invoke<SearchResult[]>('list_pinned_chunks');
    setStore('pinnedList', list);
    setStore('pinnedIds', new Set(list.map((r) => r.chunk_id)));
  } catch (err) {
    console.error('Failed to load pinned chunks', err);
  }
}
