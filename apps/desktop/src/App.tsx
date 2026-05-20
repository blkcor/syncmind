import { onMount, onCleanup, For } from 'solid-js';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { store, setStore } from './store';
import SearchTab from './components/SearchTab';
import RagLabTab from './components/RagLabTab';
import SettingsTab from './components/SettingsTab';

export default function App() {
  const tabs = [
    { key: 'search' as const, label: 'Search' },
    { key: 'rag-lab' as const, label: 'RAG Lab' },
    { key: 'settings' as const, label: 'Settings' },
  ];

  onMount(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' || (e.metaKey && e.key === 'w')) {
        e.preventDefault();
        getCurrentWindow().hide();
      }
    };
    window.addEventListener('keydown', onKeyDown);

    let unlistenNavigate: UnlistenFn | undefined;
    listen<'search' | 'rag-lab' | 'settings'>('tray-navigate', (event) => {
      setStore('activeTab', event.payload);
    }).then((unlisten) => {
      unlistenNavigate = unlisten;
    });

    onCleanup(() => {
      window.removeEventListener('keydown', onKeyDown);
      unlistenNavigate?.();
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
        {store.activeTab === 'rag-lab' && <RagLabTab />}
        {store.activeTab === 'settings' && <SettingsTab />}
      </main>
    </div>
  );
}
