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
