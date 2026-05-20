import { createSignal, onMount, onCleanup, Show, For } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { store, setStore } from '../store';
import type { ConfigPatch } from '@syncmind/types';

export default function SettingsTab() {
  const [urlError, setUrlError] = createSignal('');
  const [autoLaunch, setAutoLaunch] = createSignal(false);
  const [modelCustom, setModelCustom] = createSignal(false);
  let pollTimer: number | null = null;

  async function loadConfig() {
    try {
      const cfg = await invoke<typeof store.config>('get_config');
      setStore('config', cfg);
      setModelCustom(!['bge-m3', 'bge-small'].includes(cfg.ollama_model));
    } catch (e) {
      console.error('Failed to load config', e);
    }
  }

  async function loadIndexingStatus() {
    try {
      const status = await invoke<typeof store.indexingStatus>('get_indexing_status');
      setStore('indexingStatus', status);
    } catch (e) {
      console.error('Failed to load indexing status', e);
    }
  }

  async function loadAutoLaunch() {
    try {
      const enabled = await invoke<boolean>('is_auto_launch_enabled');
      setAutoLaunch(enabled);
    } catch (e) {
      console.error('Failed to load auto-launch', e);
    }
  }

  onMount(() => {
    loadConfig();
    loadIndexingStatus();
    loadAutoLaunch();
    pollTimer = window.setInterval(loadIndexingStatus, 5000);
  });

  onCleanup(() => {
    if (pollTimer) window.clearInterval(pollTimer);
  });

  function validateUrl(url: string): boolean {
    return url.startsWith('http://') || url.startsWith('https://');
  }

  async function saveConfig() {
    const url = store.config.ollama_url;
    if (!validateUrl(url)) {
      setUrlError('URL must start with http:// or https://');
      return;
    }
    setUrlError('');
    const patch: ConfigPatch = {
      ollama_url: store.config.ollama_url,
      ollama_model: store.config.ollama_model,
      registered_files: store.config.registered_files,
    };
    try {
      const updated = await invoke<typeof store.config>('update_config', { patch });
      setStore('config', updated);
    } catch (e) {
      console.error('Failed to update config', e);
    }
  }

  async function addFiles() {
    await invoke('set_dialog_open', { open: true });
    let selected: string | string[] | null = null;
    try {
      selected = await open({ multiple: true });
    } finally {
      await invoke('set_dialog_open', { open: false });
    }
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    const updatedFiles = [...store.config.registered_files, ...paths];
    const patch: ConfigPatch = { registered_files: updatedFiles };
    try {
      const updated = await invoke<typeof store.config>('update_config', { patch });
      setStore('config', updated);
    } catch (e) {
      console.error('Failed to add files', e);
    }
  }

  function removeFile(index: number) {
    const updated = store.config.registered_files.filter((_, i) => i !== index);
    const patch: ConfigPatch = { registered_files: updated };
    invoke<typeof store.config>('update_config', { patch })
      .then((cfg) => setStore('config', cfg))
      .catch(console.error);
  }

  async function rebuildAll() {
    if (!window.confirm('Rebuild index for all registered files?')) return;
    try {
      await invoke('trigger_reindex', { filePath: null });
      await loadIndexingStatus();
    } catch (e) {
      console.error('Reindex failed', e);
    }
  }

  async function toggleAutoLaunch() {
    const next = !autoLaunch();
    try {
      await invoke('set_auto_launch', { enabled: next });
      setAutoLaunch(next);
    } catch (e) {
      console.error('Failed to set auto-launch', e);
    }
  }

  return (
    <div class="tab-content settings-tab">
      <h2>Settings</h2>

      <div class="settings-section">
        <h3>Configuration</h3>
        <label class="field">
          <span>Ollama URL</span>
          <input
            type="text"
            value={store.config.ollama_url}
            onInput={(e) => setStore('config', 'ollama_url', e.currentTarget.value)}
          />
        </label>
        <Show when={urlError()}>
          <div class="field-error">{urlError()}</div>
        </Show>

        <label class="field">
          <span>Model</span>
          <Show
            when={!modelCustom()}
            fallback={
              <div class="model-input-row">
                <input
                  type="text"
                  value={store.config.ollama_model}
                  onInput={(e) => setStore('config', 'ollama_model', e.currentTarget.value)}
                />
                <button class="action-btn" onClick={() => setModelCustom(false)}>
                  Presets
                </button>
              </div>
            }
          >
            <select
              value={store.config.ollama_model}
              onChange={(e) => {
                const val = e.currentTarget.value;
                if (val === 'custom') {
                  setModelCustom(true);
                } else {
                  setStore('config', 'ollama_model', val);
                }
              }}
            >
              <option value="bge-m3">bge-m3</option>
              <option value="bge-small">bge-small</option>
              <option value="custom">Custom...</option>
            </select>
          </Show>
        </label>

        <div class="field read-only-field">
          <span>Transport</span>
          <span class="read-only-value">{store.config.mcp_transport}</span>
          <span class="field-note">Managed by CLI daemon</span>
        </div>

        <button class="action-btn save-btn" onClick={saveConfig}>
          Save
        </button>
      </div>

      <div class="settings-section">
        <h3>Registered Files</h3>
        <div class="file-list">
          <For each={store.config.registered_files}>
            {(path, index) => (
              <div class="file-row">
                <span class="file-path" title={path}>
                  {path.length > 60 ? '…' + path.slice(-59) : path}
                </span>
                <button class="icon-btn" onClick={() => removeFile(index())} title="Remove">
                  ✕
                </button>
              </div>
            )}
          </For>
        </div>
        <button class="action-btn" onClick={addFiles}>
          Add Files
        </button>
      </div>

      <div class="settings-section">
        <h3>Indexing Status</h3>
        <div class="status-cards">
          <div class="status-card">
            <span class="status-label">Files</span>
            <span class="status-value">{store.indexingStatus.file_count}</span>
          </div>
          <div class="status-card">
            <span class="status-label">Chunks</span>
            <span class="status-value">{store.indexingStatus.chunk_count}</span>
          </div>
          <div class="status-card">
            <span class="status-label">Last Updated</span>
            <span class="status-value">{store.indexingStatus.last_updated ?? 'Never'}</span>
          </div>
        </div>

        <Show when={store.indexingStatus.recent_errors.length > 0}>
          <div class="error-log">
            <h4>Recent Errors</h4>
            <For each={store.indexingStatus.recent_errors}>
              {(err) => (
                <div class="error-row">
                  <span class="error-path" title={err.file_path}>{err.file_path}</span>
                  <span class="error-msg">{err.message}</span>
                  <span class="error-time">{err.timestamp}</span>
                </div>
              )}
            </For>
          </div>
        </Show>

        <button class="action-btn danger-btn" onClick={rebuildAll}>
          Rebuild All
        </button>
      </div>

      <div class="settings-section">
        <h3>System</h3>
        <label class="field checkbox-field">
          <input
            type="checkbox"
            checked={autoLaunch()}
            onChange={toggleAutoLaunch}
          />
          <span>Launch at login</span>
        </label>
      </div>
    </div>
  );
}
