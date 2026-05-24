import { createSignal, onMount, onCleanup, Show, For } from 'solid-js';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

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

interface InboxEntry {
  path: string;
  size_bytes: number;
  modified_unix: number;
}

const POLL_MS = 1500;

function shortFingerprint(fp: string | null | undefined): string {
  if (!fp) return '';
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

export default function DevicesTab() {
  const [config, setConfig] = createSignal<SpineConfigView | null>(null);
  const [identity, setIdentity] = createSignal<IdentityView | null>(null);
  const [pairState, setPairState] = createSignal<PairingStateView | null>(null);
  const [pairHandle, setPairHandle] = createSignal<PairingHandleView | null>(null);
  const [inbox, setInbox] = createSignal<InboxEntry[]>([]);

  const [urlDraft, setUrlDraft] = createSignal('');
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [showUnpairConfirm, setShowUnpairConfirm] = createSignal(false);
  const [unpairClearInbox, setUnpairClearInbox] = createSignal(false);
  const [showClearInboxConfirm, setShowClearInboxConfirm] = createSignal(false);
  const [sendNoteOpen, setSendNoteOpen] = createSignal(false);
  const [sendFilename, setSendFilename] = createSignal('');
  const [sendBody, setSendBody] = createSignal('');

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

  return (
    <div class="devices-tab" style={{ padding: '16px', overflow: 'auto', height: '100%' }}>
      <h2 style={{ 'margin-top': 0 }}>Devices</h2>

      <Show when={error()}>
        {(message) => (
          <div style={{ background: '#3a1e1e', padding: '8px 12px', 'border-radius': '6px', 'margin-bottom': '12px' }}>
            {message()}
            <button style={{ float: 'right' }} onClick={() => setError(null)}>×</button>
          </div>
        )}
      </Show>

      {/* Spine URL card */}
      <section style={{ 'margin-bottom': '16px', padding: '12px', background: '#1e1e1e', 'border-radius': '8px' }}>
        <h3 style={{ 'margin-top': 0 }}>Spine server</h3>
        <Show when={config()?.plain_http}>
          <div style={{ background: '#3a3019', padding: '8px 12px', 'border-radius': '6px', 'margin-bottom': '8px' }}>
            ⚠️ Spine URL uses plain HTTP. End-to-end encryption still applies, but consider HTTPS.
          </div>
        </Show>
        <div style={{ display: 'flex', gap: '8px' }}>
          <input
            type="text"
            value={urlDraft()}
            placeholder="https://spine.example.com"
            onInput={(e) => setUrlDraft(e.currentTarget.value)}
            style={{ flex: 1, padding: '6px 8px' }}
          />
          <button disabled={busy()} onClick={saveUrl}>
            Save URL
          </button>
        </div>
        <div style={{ 'margin-top': '8px', display: 'flex', gap: '8px', 'align-items': 'center' }}>
          <button disabled={busy()} onClick={pickTrustCa}>
            Trust self-signed CA…
          </button>
          <Show when={config()?.trust_ca_path}>
            {(p) => (
              <>
                <code style={{ 'font-size': '0.9em' }}>{p()}</code>
                <button disabled={busy()} onClick={clearTrustCa}>
                  Clear
                </button>
              </>
            )}
          </Show>
        </div>
      </section>

      {/* Local identity card */}
      <section style={{ 'margin-bottom': '16px', padding: '12px', background: '#1e1e1e', 'border-radius': '8px' }}>
        <h3 style={{ 'margin-top': 0 }}>This device</h3>
        <Show when={identity()} fallback={<div>—</div>}>
          {(id) => (
            <div style={{ display: 'flex', 'flex-direction': 'column', gap: '4px' }}>
              <div>
                <strong>Fingerprint:</strong>{' '}
                <code title={id().fingerprint}>{shortFingerprint(id().fingerprint)}</code>{' '}
                <button onClick={() => copyFingerprint(id().fingerprint)}>Copy</button>
              </div>
              <div>
                <strong>UUID:</strong> <code>{id().device_uuid}</code>
              </div>
              <div>
                <strong>Type:</strong> {id().device_type}
              </div>
              <div>
                <strong>Created:</strong> {formatTimestamp(id().created_at)}
              </div>
            </div>
          )}
        </Show>
      </section>

      {/* Pairing card */}
      <section style={{ 'margin-bottom': '16px', padding: '12px', background: '#1e1e1e', 'border-radius': '8px' }}>
        <h3 style={{ 'margin-top': 0 }}>Pairing</h3>
        <Show
          when={config()?.is_paired}
          fallback={
            <div>
              <Show when={!pairHandle()}>
                <button disabled={busy() || !config()?.is_enabled} onClick={startPairing}>
                  Start pairing
                </button>
                <Show when={!config()?.is_enabled}>
                  <p style={{ color: '#999' }}>Configure a Spine URL first.</p>
                </Show>
              </Show>
              <Show when={pairHandle()}>
                {(h) => (
                  <div style={{ 'text-align': 'center' }}>
                    <img src={h().qr_png_base64} alt="pairing QR" style={{ width: '320px', height: '320px' }} />
                    <div style={{ 'font-size': '1.6em', 'font-family': 'monospace', 'margin-top': '8px' }}>
                      {h().short_code}
                    </div>
                    <div style={{ 'margin-top': '4px', color: '#999' }}>
                      Expires {formatTimestamp(h().expires_at)}
                    </div>
                    <div style={{ 'margin-top': '4px', color: '#bbb' }}>
                      State: <strong>{pairState()?.state ?? 'pending'}</strong>
                    </div>
                    <button onClick={cancelPairing} disabled={busy()} style={{ 'margin-top': '12px' }}>
                      Cancel
                    </button>
                  </div>
                )}
              </Show>
            </div>
          }
        >
          <div>
            <div>
              <strong>Peer fingerprint:</strong>{' '}
              <code title={config()?.paired_peer_fingerprint ?? ''}>
                {shortFingerprint(config()?.paired_peer_fingerprint)}
              </code>{' '}
              <button onClick={() => copyFingerprint(config()?.paired_peer_fingerprint ?? '')}>Copy</button>
            </div>
            <Show when={config()?.peer_device_id_uuid}>
              <div>
                <strong>Peer UUID:</strong> <code>{config()?.peer_device_id_uuid}</code>
              </div>
            </Show>
            <div>
              <strong>Paired at:</strong> {formatTimestamp(config()?.paired_at)}
            </div>
            <div style={{ 'margin-top': '12px', display: 'flex', gap: '8px' }}>
              <button onClick={() => setSendNoteOpen(true)} disabled={busy()}>
                Send note
              </button>
              <button onClick={pullNow} disabled={busy()}>
                Pull now
              </button>
              <button onClick={() => setShowUnpairConfirm(true)} disabled={busy()} style={{ color: '#f88' }}>
                Unpair
              </button>
            </div>
          </div>
        </Show>
      </section>

      {/* Inbox card */}
      <section style={{ 'margin-bottom': '16px', padding: '12px', background: '#1e1e1e', 'border-radius': '8px' }}>
        <h3 style={{ 'margin-top': 0 }}>Sync inbox</h3>
        <div>
          <div>
            <strong>Files:</strong> {inboxStats().count}
          </div>
          <div>
            <strong>Total size:</strong> {formatBytes(inboxStats().totalBytes)}
          </div>
          <div>
            <strong>Latest:</strong> {inboxStats().latest}
          </div>
        </div>
        <div style={{ 'margin-top': '12px' }}>
          <button
            onClick={() => setShowClearInboxConfirm(true)}
            disabled={busy() || inboxStats().count === 0}
          >
            Empty inbox…
          </button>
        </div>
      </section>

      {/* Unpair confirm dialog */}
      <Show when={showUnpairConfirm()}>
        <div
          style={{
            position: 'fixed',
            inset: '0',
            background: 'rgba(0,0,0,0.6)',
            display: 'flex',
            'align-items': 'center',
            'justify-content': 'center',
          }}
        >
          <div style={{ background: '#2a2a2a', padding: '24px', 'border-radius': '8px', 'max-width': '480px' }}>
            <h3 style={{ 'margin-top': 0 }}>Unpair this device?</h3>
            <p>This will:</p>
            <ul>
              <li>Revoke the current authentication token on the Spine</li>
              <li>Erase the cached sync_key from your OS keychain</li>
              <li>Disconnect the live notification channel</li>
              <li>Preserve the sync-inbox files unless you check the box below</li>
            </ul>
            <label style={{ display: 'block', 'margin-top': '12px' }}>
              <input
                type="checkbox"
                checked={unpairClearInbox()}
                onChange={(e) => setUnpairClearInbox(e.currentTarget.checked)}
              />{' '}
              Also empty sync-inbox (cannot be undone)
            </label>
            <div style={{ 'margin-top': '16px', display: 'flex', 'justify-content': 'flex-end', gap: '8px' }}>
              <button onClick={() => setShowUnpairConfirm(false)} disabled={busy()}>
                Cancel
              </button>
              <button onClick={performUnpair} disabled={busy()} style={{ color: '#f88' }}>
                Unpair
              </button>
            </div>
          </div>
        </div>
      </Show>

      {/* Clear inbox confirm dialog */}
      <Show when={showClearInboxConfirm()}>
        <div
          style={{
            position: 'fixed',
            inset: '0',
            background: 'rgba(0,0,0,0.6)',
            display: 'flex',
            'align-items': 'center',
            'justify-content': 'center',
          }}
        >
          <div style={{ background: '#2a2a2a', padding: '24px', 'border-radius': '8px', 'max-width': '420px' }}>
            <h3 style={{ 'margin-top': 0 }}>Empty sync inbox?</h3>
            <p>
              {inboxStats().count} file(s), {formatBytes(inboxStats().totalBytes)} will be deleted from{' '}
              <code>sync-inbox/</code>. The indexed content remains in the vector store.
            </p>
            <div style={{ 'margin-top': '16px', display: 'flex', 'justify-content': 'flex-end', gap: '8px' }}>
              <button onClick={() => setShowClearInboxConfirm(false)} disabled={busy()}>
                Cancel
              </button>
              <button onClick={performClearInbox} disabled={busy()} style={{ color: '#f88' }}>
                Empty inbox
              </button>
            </div>
          </div>
        </div>
      </Show>

      {/* Send note dialog */}
      <Show when={sendNoteOpen()}>
        <div
          style={{
            position: 'fixed',
            inset: '0',
            background: 'rgba(0,0,0,0.6)',
            display: 'flex',
            'align-items': 'center',
            'justify-content': 'center',
          }}
        >
          <div style={{ background: '#2a2a2a', padding: '24px', 'border-radius': '8px', 'max-width': '560px', width: '90%' }}>
            <h3 style={{ 'margin-top': 0 }}>Send note to paired device</h3>
            <label style={{ display: 'block', 'margin-bottom': '8px' }}>
              Filename
              <input
                type="text"
                value={sendFilename()}
                onInput={(e) => setSendFilename(e.currentTarget.value)}
                placeholder="note.md"
                style={{ width: '100%', padding: '6px 8px', 'margin-top': '4px' }}
              />
            </label>
            <label style={{ display: 'block' }}>
              Body
              <textarea
                rows={8}
                value={sendBody()}
                onInput={(e) => setSendBody(e.currentTarget.value)}
                placeholder="Type your note…"
                style={{ width: '100%', padding: '6px 8px', 'margin-top': '4px' }}
              />
            </label>
            <div style={{ 'margin-top': '16px', display: 'flex', 'justify-content': 'flex-end', gap: '8px' }}>
              <button onClick={() => setSendNoteOpen(false)} disabled={busy()}>
                Cancel
              </button>
              <button onClick={sendNote} disabled={busy() || !sendBody()}>
                Send
              </button>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
}
