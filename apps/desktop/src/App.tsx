import { onMount, onCleanup, For } from 'solid-js';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { store, setStore } from './store';
import SearchTab from './components/SearchTab';
import RagLabTab from './components/RagLabTab';
import SettingsTab from './components/SettingsTab';
import PinnedTab from './components/PinnedTab';
import DevicesTab from './components/DevicesTab';
import { refreshPinnedList } from './pins';

export default function App() {
  const tabs = [
    { key: 'search' as const, label: 'Search' },
    { key: 'pinned' as const, label: 'Pinned' },
    { key: 'rag-lab' as const, label: 'RAG Lab' },
    { key: 'settings' as const, label: 'Settings' },
    { key: 'devices' as const, label: 'Devices' },
  ];

  onMount(() => {
    // Hydrate the pinned set once on startup so search results can show
    // the correct icon state without waiting for the user to open the tab.
    refreshPinnedList();

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' || (e.metaKey && e.key === 'w')) {
        e.preventDefault();
        getCurrentWindow().hide();
        return;
      }
      // Cmd+Shift+P opens the Pinned tab from anywhere in the palette.
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === 'p') {
        e.preventDefault();
        setStore('activeTab', 'pinned');
      }
    };
    window.addEventListener('keydown', onKeyDown);

    let unlistenNavigate: UnlistenFn | undefined;
    let unlistenReindex: UnlistenFn | undefined;
    let unlistenPairing: UnlistenFn | undefined;

    listen<'search' | 'rag-lab' | 'settings' | 'pinned' | 'devices'>('tray-navigate', (event) => {
      setStore('activeTab', event.payload);
    }).then((unlisten) => {
      unlistenNavigate = unlisten;
    });

    listen<{ current: number; total: number; file_path: string }>('reindex://progress', (event) => {
      setStore('reindexProgress', event.payload);
    });

    listen('reindex://complete', () => {
      setStore('reindexProgress', null);
    });

    listen<{ step: string }>('spine://pairing/step', (event) => {
      setStore('pairingStep', event.payload.step);
    });

    onCleanup(() => {
      window.removeEventListener('keydown', onKeyDown);
      unlistenNavigate?.();
      unlistenReindex?.();
      unlistenPairing?.();
    });
  });

  return (
    <div class="app">
      <nav class="tab-bar" data-tauri-drag-region>
        <For each={tabs}>
          {(tab) => (
            <button
              class="tab-button"
              classList={{ active: store.activeTab === tab.key }}
              onClick={() => setStore('activeTab', tab.key)}
            >
              {tab.label}
            </button>
          )}
        </For>
      </nav>
      <main class="main">
        {store.activeTab === 'search' && <SearchTab />}
        {store.activeTab === 'pinned' && <PinnedTab />}
        {store.activeTab === 'rag-lab' && <RagLabTab />}
        {store.activeTab === 'settings' && <SettingsTab />}
        {store.activeTab === 'devices' && <DevicesTab />}
      </main>
    </div>
  );
}
