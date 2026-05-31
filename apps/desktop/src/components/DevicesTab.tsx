import { createSignal, onMount, onCleanup, Show, For } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { store } from '../store';

interface SpineConfigView {
  url: string | null;
  trust_ca_path: string | null;
  paired_peer_fingerprint: string | null;
  paired_peer_device_type: string | null;
  paired_at: string | null;
  peer_device_id_uuid: string | null;
  is_enabled: boolean;
  is_paired: boolean;
  plain_http: boolean;
}

interface IdentityView {
  fingerprint: string;
  device_uuid: string;
  device_type: string;
  created_at: string;
}

interface PairingHandleView {
  session_id: string;
  short_code: string;
  qr_png_base64: string;
  expires_at: string;
}

interface PairingStateView {
  state: 'idle' | 'pending' | 'paired' | 'expired' | 'failed' | 'cancelled' | string;
  session_id: string | null;
  peer_fingerprint: string | null;
  peer_device_id: string | null;
  error_code: string | null;
  error_message: string | null;
}

interface CompletePairingResult {
  peer_fingerprint: string;
  peer_device_id: string | null;
  config: SpineConfigView;
}

interface InboxEntry {
  path: string;
  size_bytes: number;
  modified_unix: number;
}

const POLL_MS = 1500;

function shortFingerprint(fp: string | null | undefined): string {
  if (!fp) return '—';
  return `${fp.slice(0, 8)}…${fp.slice(-4)}`;
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function formatTimestamp(ts: string | null | undefined): string {
  if (!ts) return '—';
  try {
    return new Date(ts).toLocaleString();
  } catch {
    return ts;
  }
}

function humanizeStatus(status: string | null | undefined): string {
  if (!status) return 'idle';
  return status.replace(/_/g, ' ');
}

function dotTone(status: string | null | undefined, liveValues: string[]): 'live' | 'warn' | 'muted' {
  if (!status) return 'muted';
  if (liveValues.includes(status)) return 'live';
  if (['reconnecting', 'offline', 'failed', 'expired', 'cancelled'].includes(status)) return 'warn';
  return 'muted';
}

const PAIRING_STEPS: { key: string; label: string }[] = [
  { key: 'contacting_server', label: 'Contacting server' },
  { key: 'deriving_keys', label: 'Deriving encryption keys' },
  { key: 'saving_keychain', label: 'Saving to keychain' },
  { key: 'updating_config', label: 'Updating configuration' },
];

function pairingStepLabel(step: string): string {
  const found = PAIRING_STEPS.find((s) => s.key === step);
  return found ? found.label : step;
}

function pairingStepIndex(step: string): number {
  const idx = PAIRING_STEPS.findIndex((s) => s.key === step);
  return idx >= 0 ? idx : 999;
}

export default function DevicesTab() {
  const [config, setConfig] = createSignal<SpineConfigView | null>(null);
  const [identity, setIdentity] = createSignal<IdentityView | null>(null);
  const [pairState, setPairState] = createSignal<PairingStateView | null>(null);
  const [pairHandle, setPairHandle] = createSignal<PairingHandleView | null>(null);
  const [inbox, setInbox] = createSignal<InboxEntry[]>([]);
  const [connectionStatus, setConnectionStatus] = createSignal('disabled');

  const [urlDraft, setUrlDraft] = createSignal('');
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [showUnpairConfirm, setShowUnpairConfirm] = createSignal(false);
  const [unpairClearInbox, setUnpairClearInbox] = createSignal(false);
  const [showClearInboxConfirm, setShowClearInboxConfirm] = createSignal(false);
  const [sendNoteOpen, setSendNoteOpen] = createSignal(false);
  const [sendFilename, setSendFilename] = createSignal('');
  const [sendBody, setSendBody] = createSignal('');
  const [joinShortCode, setJoinShortCode] = createSignal('');

  let pollTimer: number | null = null;

  async function refresh() {
    try {
      const [cfg, id, ps, inb] = await Promise.all([
        invoke<SpineConfigView>('spine_get_config'),
        invoke<IdentityView>('spine_get_identity'),
        invoke<PairingStateView>('spine_pair_status'),
        invoke<InboxEntry[]>('spine_list_inbox').catch(() => [] as InboxEntry[]),
      ]);
      setConfig(cfg);
      setIdentity(id);
      setPairState(ps);
      setInbox(inb);
      if (urlDraft() === '' && cfg.url) setUrlDraft(cfg.url);
    } catch (e) {
      console.error('devices refresh failed', e);
    }
  }

  onMount(() => {
    refresh();
    pollTimer = window.setInterval(refresh, POLL_MS);
    let unlistenStatus: UnlistenFn | undefined;
    listen<string>('spine://status', (event) => {
      setConnectionStatus(event.payload);
    }).then((unlisten) => {
      unlistenStatus = unlisten;
    });
    onCleanup(() => {
      unlistenStatus?.();
    });
  });

  onCleanup(() => {
    if (pollTimer) window.clearInterval(pollTimer);
  });

  async function saveUrl() {
    setBusy(true);
    setError(null);
    try {
      const updated = await invoke<SpineConfigView>('spine_set_url', { url: urlDraft() });
      setConfig(updated);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function pickTrustCa() {
    try {
      const picked = await open({
        multiple: false,
        directory: false,
        filters: [{ name: 'PEM certificate', extensions: ['pem', 'crt', 'cer'] }],
      });
      if (!picked || Array.isArray(picked)) return;
      setBusy(true);
      setError(null);
      const updated = await invoke<SpineConfigView>('spine_set_trust_ca', { path: picked });
      setConfig(updated);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function clearTrustCa() {
    setBusy(true);
    setError(null);
    try {
      const updated = await invoke<SpineConfigView>('spine_set_trust_ca', { path: null });
      setConfig(updated);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function startPairing() {
    setBusy(true);
    setError(null);
    try {
      const handle = await invoke<PairingHandleView>('spine_start_pairing');
      setPairHandle(handle);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function cancelPairing() {
    setBusy(true);
    setError(null);
    try {
      await invoke('spine_cancel_pairing');
      setPairHandle(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function completePairingShortCode() {
    setBusy(true);
    setError(null);
    try {
      const result = await invoke<CompletePairingResult>('spine_complete_pairing_short_code', {
        shortCode: joinShortCode(),
      });
      setConfig(result.config);
      setPairHandle(null);
      setJoinShortCode('');
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function performUnpair() {
    setBusy(true);
    setError(null);
    try {
      const updated = await invoke<SpineConfigView>('spine_unpair', {
        clearInbox: unpairClearInbox(),
      });
      setConfig(updated);
      setPairHandle(null);
      setShowUnpairConfirm(false);
      setUnpairClearInbox(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function performClearInbox() {
    setBusy(true);
    setError(null);
    try {
      await invoke('spine_clear_inbox');
      setShowClearInboxConfirm(false);
      const inb = await invoke<InboxEntry[]>('spine_list_inbox');
      setInbox(inb);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function sendNote() {
    if (!sendBody()) {
      setError('note body is empty');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await invoke('spine_send_note', {
        filename: sendFilename() || 'note.md',
        contentUtf8: sendBody(),
        sourcePath: null,
      });
      setSendNoteOpen(false);
      setSendFilename('');
      setSendBody('');
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function pullNow() {
    setBusy(true);
    setError(null);
    try {
      const result = await invoke<{ processed: number; failed: number }>('spine_pull_bundles');
      setError(`pulled ${result.processed} bundle(s), ${result.failed} failed`);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function copyFingerprint(fp: string) {
    try {
      await navigator.clipboard.writeText(fp);
    } catch {
      /* ignore */
    }
  }

  const inboxStats = () => {
    const list = inbox();
    const totalBytes = list.reduce((acc, e) => acc + e.size_bytes, 0);
    const latest = list[0]?.modified_unix ?? 0;
    return {
      count: list.length,
      totalBytes,
      latest: latest > 0 ? new Date(latest * 1000).toLocaleString() : '—',
    };
  };

  const connTone = () => dotTone(connectionStatus(), ['connected']);
  const pairTone = () => dotTone(pairState()?.state, ['paired']);

  return (
    <div class="tab-content devices-tab">
      <h2>Spine</h2>

      <Show when={error()}>
        {(message) => (
          <div class="devices-alert">
            <div>{message()}</div>
            <button class="devices-alert-dismiss" onClick={() => setError(null)}>×</button>
          </div>
        )}
      </Show>

      <Show when={config()?.plain_http}>
        <div class="devices-alert devices-alert-warn">
          <div>Plain HTTP is active. End-to-end encryption still applies, but transport metadata is less protected.</div>
        </div>
      </Show>

      <div class="devices-section">
        <h3>Status</h3>
        <div class="devices-status-row">
          <div class="devices-status-card">
            <span class="devices-status-label">Connection</span>
            <span class="devices-status-value">
              <span class={`devices-status-dot devices-status-dot-${connTone()}`} />
              {humanizeStatus(connectionStatus())}
            </span>
          </div>
          <div class="devices-status-card">
            <span class="devices-status-label">Pairing</span>
            <span class="devices-status-value">
              <span class={`devices-status-dot devices-status-dot-${pairTone()}`} />
              {humanizeStatus(pairState()?.state)}
            </span>
          </div>
          <div class="devices-status-card">
            <span class="devices-status-label">Inbox</span>
            <span class="devices-status-value devices-status-value-mono">
              {inboxStats().count} · {formatBytes(inboxStats().totalBytes)}
            </span>
          </div>
        </div>
      </div>

      <div class="devices-section">
        <h3>Server</h3>
        <div class="devices-input-row">
          <input
            class="devices-input"
            type="text"
            value={urlDraft()}
            placeholder="https://spine.example.com"
            onInput={(e) => setUrlDraft(e.currentTarget.value)}
          />
          <button class="action-btn" disabled={busy()} onClick={saveUrl}>
            Save
          </button>
        </div>
        <div class="devices-inline-row">
          <button class="action-btn" disabled={busy()} onClick={pickTrustCa}>
            Trust self-signed CA…
          </button>
          <Show when={config()?.trust_ca_path}>
            {(p) => (
              <>
                <span class="devices-trust-path" title={p()}>{p()}</span>
                <button class="action-btn" disabled={busy()} onClick={clearTrustCa}>
                  Clear
                </button>
              </>
            )}
          </Show>
        </div>
      </div>

      <div class="devices-section">
        <h3>Identity</h3>
        <Show when={identity()} fallback={<div class="empty-state">—</div>}>
          {(id) => (
            <div class="devices-meta-list">
              <div class="devices-meta-row">
                <span class="devices-meta-label">Fingerprint</span>
                <span class="devices-meta-value" title={id().fingerprint}>{shortFingerprint(id().fingerprint)}</span>
                <button class="action-btn" onClick={() => copyFingerprint(id().fingerprint)}>Copy</button>
              </div>
              <div class="devices-meta-row">
                <span class="devices-meta-label">UUID</span>
                <span class="devices-meta-value">{id().device_uuid}</span>
              </div>
              <div class="devices-meta-row">
                <span class="devices-meta-label">Type</span>
                <span class="devices-meta-value devices-meta-value-plain">{id().device_type}</span>
              </div>
              <div class="devices-meta-row">
                <span class="devices-meta-label">Created</span>
                <span class="devices-meta-value devices-meta-value-plain">{formatTimestamp(id().created_at)}</span>
              </div>
            </div>
          )}
        </Show>
        <div class="field-note">
          The device key lives locally in the OS keychain. Pairing only shares public identity.
        </div>
      </div>

      <div class="devices-section">
        <h3>Pairing</h3>
        <Show
          when={config()?.is_paired}
          fallback={
            <Show
              when={pairHandle()}
              fallback={
                <div class="devices-pair-empty">
                  <div class="devices-pair-empty-copy">
                    Generate a QR payload and a 6-digit short code, or enter the code from another desktop.
                  </div>
                  <div class="devices-action-row">
                    <button class="action-btn" disabled={busy() || !config()?.is_enabled} onClick={startPairing}>
                      Start pairing
                    </button>
                    <Show when={!config()?.is_enabled}>
                      <span class="field-note">Configure a Spine URL first.</span>
                    </Show>
                  </div>
                  <div class="devices-join-row">
                    <input
                      class="devices-input devices-short-code-input"
                      type="text"
                      inputMode="numeric"
                      value={joinShortCode()}
                      placeholder="123-456"
                      maxLength={7}
                      onInput={(e) => setJoinShortCode(e.currentTarget.value)}
                    />
                    <button
                      class="action-btn"
                      disabled={busy() || !config()?.is_enabled || joinShortCode().trim().length < 6}
                      onClick={completePairingShortCode}
                    >
                      Join pairing
                    </button>
                  </div>
                  <Show when={store.pairingStep}>
                    <div class="pairing-progress-steps">
                      <For each={PAIRING_STEPS}>
                        {(s) => {
                          const curIdx = () => pairingStepIndex(store.pairingStep!);
                          const myIdx = PAIRING_STEPS.findIndex((x) => x.key === s.key);
                          const done = () => myIdx < curIdx() || store.pairingStep === 'completed';
                          const active = () => myIdx === curIdx() && store.pairingStep !== 'completed';
                          return (
                            <div
                              class="pairing-step"
                              classList={{
                                active: active(),
                                done: done(),
                              }}
                            >
                              <span class="pairing-step-icon">
                                {done() ? '✓' : active() ? '●' : '○'}
                              </span>
                              <span>{s.label}</span>
                            </div>
                          );
                        }}
                      </For>
                    </div>
                  </Show>
                </div>
              }
            >
              {(h) => (
                <div class="devices-pair-stage">
                  <div class="devices-qr">
                    <img src={h().qr_png_base64} alt="pairing QR" />
                  </div>
                  <div class="devices-pair-copy">
                    <span class="devices-status-label">Short code</span>
                    <div class="devices-short-code-row">
                      <div class="devices-short-code">{h().short_code}</div>
                      <button class="action-btn" onClick={() => copyFingerprint(h().short_code)}>
                        Copy
                      </button>
                    </div>
                    <div class="devices-pair-meta-row">
                      <span>Session {h().session_id.slice(0, 8)}…</span>
                      <span>Expires {formatTimestamp(h().expires_at)}</span>
                      <span>State: {humanizeStatus(pairState()?.state ?? 'pending')}</span>
                    </div>
                    <div class="devices-action-row">
                      <button class="action-btn" onClick={cancelPairing} disabled={busy()}>
                        Cancel
                      </button>
                    </div>
                  </div>
                </div>
              )}
            </Show>
          }
        >
          <div class="devices-meta-list">
            <div class="devices-meta-row">
              <span class="devices-meta-label">Peer fingerprint</span>
              <span class="devices-meta-value" title={config()?.paired_peer_fingerprint ?? ''}>
                {shortFingerprint(config()?.paired_peer_fingerprint)}
              </span>
              <button class="action-btn" onClick={() => copyFingerprint(config()?.paired_peer_fingerprint ?? '')}>
                Copy
              </button>
            </div>
            <Show when={config()?.peer_device_id_uuid}>
              <div class="devices-meta-row">
                <span class="devices-meta-label">Peer UUID</span>
                <span class="devices-meta-value">{config()?.peer_device_id_uuid}</span>
              </div>
            </Show>
            <div class="devices-meta-row">
              <span class="devices-meta-label">Paired at</span>
              <span class="devices-meta-value devices-meta-value-plain">{formatTimestamp(config()?.paired_at)}</span>
            </div>
            <div class="devices-meta-row">
              <span class="devices-meta-label">Live sync</span>
              <span class="devices-meta-value devices-meta-value-plain">
                <span class={`devices-status-dot devices-status-dot-${connTone()}`} />
                {humanizeStatus(connectionStatus())}
              </span>
            </div>
          </div>
          <div class="devices-action-row">
            <button class="action-btn" onClick={() => setSendNoteOpen(true)} disabled={busy()}>
              Send note
            </button>
            <button class="action-btn" onClick={pullNow} disabled={busy()}>
              Pull now
            </button>
            <button class="action-btn danger-btn" onClick={() => setShowUnpairConfirm(true)} disabled={busy()}>
              Unpair
            </button>
          </div>
        </Show>
      </div>

      <div class="devices-section">
        <h3>Inbox</h3>
        <div class="devices-status-row">
          <div class="devices-status-card">
            <span class="devices-status-label">Files</span>
            <span class="devices-status-value devices-status-value-mono">{inboxStats().count}</span>
          </div>
          <div class="devices-status-card">
            <span class="devices-status-label">Total size</span>
            <span class="devices-status-value devices-status-value-mono">{formatBytes(inboxStats().totalBytes)}</span>
          </div>
          <div class="devices-status-card">
            <span class="devices-status-label">Latest write</span>
            <span class="devices-status-value devices-status-value-mono">{inboxStats().latest}</span>
          </div>
        </div>
        <div class="field-note">
          Decrypted notes are materialized into <code>sync-inbox/</code> and indexed locally after write completion.
        </div>
        <div class="devices-action-row">
          <button
            class="action-btn"
            onClick={() => setShowClearInboxConfirm(true)}
            disabled={busy() || inboxStats().count === 0}
          >
            Empty inbox…
          </button>
        </div>
      </div>

      <Show when={showUnpairConfirm()}>
        <div class="devices-modal-backdrop">
          <div class="devices-modal">
            <h3>Unpair this device?</h3>
            <p>This will:</p>
            <ul>
              <li>Revoke the current authentication token on the Spine</li>
              <li>Erase the cached sync_key from your OS keychain</li>
              <li>Disconnect the live notification channel</li>
              <li>Preserve the sync-inbox files unless you check the box below</li>
            </ul>
            <label class="devices-checkbox-row">
              <input
                type="checkbox"
                checked={unpairClearInbox()}
                onChange={(e) => setUnpairClearInbox(e.currentTarget.checked)}
              />
              Also empty sync-inbox (cannot be undone)
            </label>
            <div class="devices-modal-actions">
              <button class="action-btn" onClick={() => setShowUnpairConfirm(false)} disabled={busy()}>
                Cancel
              </button>
              <button class="action-btn danger-btn" onClick={performUnpair} disabled={busy()}>
                Unpair
              </button>
            </div>
          </div>
        </div>
      </Show>

      <Show when={showClearInboxConfirm()}>
        <div class="devices-modal-backdrop">
          <div class="devices-modal">
            <h3>Empty sync inbox?</h3>
            <p>
              {inboxStats().count} file(s), {formatBytes(inboxStats().totalBytes)} will be deleted from{' '}
              <code>sync-inbox/</code>. The indexed content remains in the vector store.
            </p>
            <div class="devices-modal-actions">
              <button class="action-btn" onClick={() => setShowClearInboxConfirm(false)} disabled={busy()}>
                Cancel
              </button>
              <button class="action-btn danger-btn" onClick={performClearInbox} disabled={busy()}>
                Empty inbox
              </button>
            </div>
          </div>
        </div>
      </Show>

      <Show when={sendNoteOpen()}>
        <div class="devices-modal-backdrop">
          <div class="devices-modal devices-modal-wide">
            <h3>Send note to paired device</h3>
            <label class="devices-form-field">
              <span>Filename</span>
              <input
                class="devices-input"
                type="text"
                value={sendFilename()}
                onInput={(e) => setSendFilename(e.currentTarget.value)}
                placeholder="note.md"
              />
            </label>
            <label class="devices-form-field">
              <span>Body</span>
              <textarea
                class="devices-textarea"
                rows={8}
                value={sendBody()}
                onInput={(e) => setSendBody(e.currentTarget.value)}
                placeholder="Type your note…"
              />
            </label>
            <div class="devices-modal-actions">
              <button class="action-btn" onClick={() => setSendNoteOpen(false)} disabled={busy()}>
                Cancel
              </button>
              <button class="action-btn" onClick={sendNote} disabled={busy() || !sendBody()}>
                Send
              </button>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
}
