import { Component, useCallback, useEffect, useRef, useState } from 'react';
import * as Crypto from 'expo-crypto';
import {
  Alert,
  type GestureResponderEvent,
  Keyboard,
  KeyboardAvoidingView,
  Platform,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  TouchableOpacity,
  TouchableWithoutFeedback,
  View,
} from 'react-native';
import { VoiceRecorder } from '@/src/capture/VoiceRecorder';

import { PairingScanner } from '@/src/pairing/scanner';
import { useAppStore } from '@/src/store';
import { ensureIdentity } from '@/src/crypto/identity';
import {
  createCaptureAudioPayload,
  createCaptureTextPayload,
  encryptCaptureAudio,
  encryptCaptureText,
} from '@/src/crypto/bundle';
import { getRestoredPairingState } from '@/src/spine/session';
import {
  CAPTURE_AUDIO_CONTENT_TYPE,
  enqueueOutboxItem,
  flushOutbox,
  getRecentOutboxStatuses,
  type OutboxState,
  type OutboxStatusRow,
  QueueFullError,
  subscribeToOutboxChanges,
} from '@/src/outbox/service';
import {
  type AudioCaptureReadResult,
} from '@/src/capture/audio';

class CaptureErrorBoundary extends Component<
  { children: React.ReactNode },
  { hasError: boolean; errorDump: string }
> {
  constructor(props: { children: React.ReactNode }) {
    super(props);
    this.state = { hasError: false, errorDump: '' };
  }

  static getDerivedStateFromError(err: Error) {
    return { hasError: true, errorDump: `${err.name}: ${err.message}\n${err.stack ?? ''}` };
  }

  componentDidCatch(err: Error) {
    console.error('CaptureScreen error boundary caught:', err);
  }

  render() {
    if (this.state.hasError) {
      return (
        <View style={{ flex: 1, justifyContent: 'center', alignItems: 'center', padding: 24, backgroundColor: '#fff' }}>
          <Text style={{ fontSize: 18, fontWeight: '700', color: '#dc2626', marginBottom: 12 }}>
            Capture Error
          </Text>
          <Text style={{ fontSize: 12, color: '#6b7280', textAlign: 'center', fontFamily: 'monospace' }}>
            {this.state.errorDump}
          </Text>
        </View>
      );
    }
    return this.props.children;
  }
}

const MAX_CAPTURE_TEXT_CHARS = 50_000;
const VOICE_SWIPE_THRESHOLD_PX = 48;

type CaptureMode = 'text' | 'voice';

function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return "just now";
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  return `${Math.floor(hr / 24)}d ago`;
}

function shortFingerprint(fingerprint: string | null): string {
  if (!fingerprint) return "Unknown peer";
  const normalized = fingerprint.startsWith("sha256:")
    ? fingerprint.slice("sha256:".length)
    : fingerprint;
  if (normalized.length <= 12) return normalized;
  return `${normalized.slice(0, 8)}...${normalized.slice(-4)}`;
}

function statusIcon(state: OutboxState): string {
  switch (state) {
    case "done":
      return "●";
    case "sending":
      return "○";
    case "pending":
      return "○";
    case "failed":
      return "●";
  }
}

function statusText(state: OutboxState): string {
  switch (state) {
    case "done":
      return "Sent";
    case "sending":
      return "Sending...";
    case "pending":
      return "Queued";
    case "failed":
      return "Failed";
  }
}

export default function CaptureScreen() {
  const isPaired = useAppStore((state) => state.isPaired);
  const peerDeviceFingerprint = useAppStore(
    (state) => state.peerDeviceFingerprint,
  );
  const connectionStatus = useAppStore((state) => state.connectionStatus);
  const showFirstCaptureGuide = useAppStore((state) => state.showFirstCaptureGuide);
  const dismissFirstCaptureGuide = useAppStore(
    (state) => state.dismissFirstCaptureGuide,
  );
  const [note, setNote] = useState('');
  const [captureMode, setCaptureMode] = useState<CaptureMode>('text');
  const [outboxStatusRows, setOutboxStatusRows] = useState<OutboxStatusRow[]>([]);
  const textInputRef = useRef<TextInput>(null);
  const voiceSwipeStartY = useRef<number | null>(null);
  const isTooLong = note.length > MAX_CAPTURE_TEXT_CHARS;
  const canSend = note.trim().length > 0 && !isTooLong;
  const hasQueuedRows = outboxStatusRows.some((row) =>
    row.state === "pending" || row.state === "sending" || row.state === "failed"
  );
  const captureStatusLabel =
    connectionStatus === "connected"
      ? "Connected"
      : connectionStatus === "error"
        ? "Pairing invalid"
        : hasQueuedRows
          ? "Queued locally"
          : "Queued locally";
  const captureStatusColor =
    connectionStatus === "connected"
      ? "#16a34a"
      : connectionStatus === "error"
        ? "#dc2626"
        : "#6b7280";

  const refreshOutboxStatuses = useCallback(async () => {
    if (!isPaired) {
      setOutboxStatusRows([]);
      return;
    }

    setOutboxStatusRows(await getRecentOutboxStatuses(3));
  }, [isPaired]);

  useEffect(() => {
    void refreshOutboxStatuses();

    if (!isPaired) {
      return;
    }

    const unsubscribe = subscribeToOutboxChanges(() => {
      void refreshOutboxStatuses();
    });
    const interval = setInterval(() => {
      void refreshOutboxStatuses();
    }, 10_000);

    return () => {
      clearInterval(interval);
      unsubscribe();
    };
  }, [isPaired, refreshOutboxStatuses]);

  useEffect(() => {
    if (!isPaired || captureMode !== 'text') {
      return;
    }

    const timer = setTimeout(() => {
      textInputRef.current?.focus();
    }, 100);

    return () => clearTimeout(timer);
  }, [isPaired, captureMode]);

  const handleSend = async () => {
    const trimmed = note.trim();
    if (!canSend || !trimmed) return;

    const state = getRestoredPairingState();
    if (!state) return;
    const clientDeviceFingerprint = await ensureIdentity();

    const payload = createCaptureTextPayload({
      id: Crypto.randomUUID(),
      text: trimmed,
      client_ts: new Date().toISOString(),
      client_device_fingerprint: clientDeviceFingerprint,
    });

    try {
      const encrypted = await encryptCaptureText(payload, state);
      await enqueueOutboxItem(encrypted.id, encrypted.blob, trimmed);
    } catch (err) {
      if (err instanceof QueueFullError) {
        Alert.alert("Queue Full", err.message);
      }
      return;
    }

    setNote('');
    Keyboard.dismiss();

    // Best-effort flush — don't block UI on network success.
    void refreshOutboxStatuses();
    void flushOutbox()
      .catch(() => {})
      .finally(() => {
        void refreshOutboxStatuses();
      });
  };

  const enqueueAudioClip = useCallback(async (clip: AudioCaptureReadResult) => {
    if (clip.status === 'clip-too-long') {
      Alert.alert('Clip too long', 'Try a shorter recording.');
      return;
    }
    if (clip.status === 'missing-uri') {
      Alert.alert('Recording Error', 'Could not read the audio capture.');
      return;
    }

    const state = getRestoredPairingState();
    if (!state) return;
    const clientDeviceFingerprint = await ensureIdentity();
    const payload = createCaptureAudioPayload({
      id: Crypto.randomUUID(),
      audio_base64: clip.audioBase64,
      duration_ms: clip.durationMs,
      client_ts: new Date().toISOString(),
      client_device_fingerprint: clientDeviceFingerprint,
    });

    try {
      const encrypted = await encryptCaptureAudio(payload, state);
      await enqueueOutboxItem(
        encrypted.id,
        encrypted.blob,
        'Audio capture',
        CAPTURE_AUDIO_CONTENT_TYPE,
      );
    } catch (err) {
      if (err instanceof QueueFullError) {
        Alert.alert('Queue Full', err.message);
      }
      return;
    }

    void refreshOutboxStatuses();
    void flushOutbox()
      .catch(() => {})
      .finally(() => {
        void refreshOutboxStatuses();
      });
  }, [refreshOutboxStatuses]);

  const handleVoiceSwipeStart = (event: GestureResponderEvent) => {
    voiceSwipeStartY.current = event.nativeEvent.pageY;
  };

  const handleVoiceSwipeEnd = (event: GestureResponderEvent) => {
    const startY = voiceSwipeStartY.current;
    voiceSwipeStartY.current = null;
    if (startY == null) return;
    if (startY - event.nativeEvent.pageY >= VOICE_SWIPE_THRESHOLD_PX) {
      setCaptureMode('voice');
    }
  };

  if (!isPaired) {
    return (
      <CaptureErrorBoundary>
        <View style={styles.unpairedContainer}>
          <Text style={styles.unpairedHint}>
            Pair with a desktop to start capturing
          </Text>
          <PairingScanner />
        </View>
      </CaptureErrorBoundary>
    );
  }

  return (
    <CaptureErrorBoundary>
    <KeyboardAvoidingView
      style={styles.flex}
      behavior={Platform.OS === 'ios' ? 'padding' : undefined}
      keyboardVerticalOffset={90}
    >
      <TouchableWithoutFeedback onPress={Keyboard.dismiss}>
        <View style={styles.container}>
          {showFirstCaptureGuide ? (
            <View style={styles.guide}>
              <Text style={styles.guideText}>
                Send your first note! Type anything and hit Send - your desktop will
                index it.
              </Text>
              <TouchableOpacity style={styles.guideButton} onPress={dismissFirstCaptureGuide}>
                <Text style={styles.guideButtonText}>Got it</Text>
              </TouchableOpacity>
            </View>
          ) : null}

          <ScrollView
            style={styles.inputScroll}
            contentContainerStyle={styles.inputScrollContent}
            keyboardDismissMode="on-drag"
            keyboardShouldPersistTaps="handled"
          >
            <View style={styles.statusRow}>
              <View
                style={[
                  styles.statusDot,
                  { backgroundColor: captureStatusColor },
                ]}
              />
              <Text style={styles.statusText}>{captureStatusLabel}</Text>
              <Text style={styles.statusPeer} numberOfLines={1}>
                {shortFingerprint(peerDeviceFingerprint)}
              </Text>
            </View>
            {captureMode === 'text' ? (
              <TextInput
                ref={textInputRef}
                autoFocus
                multiline
                placeholder="Capture a note"
                style={styles.input}
                textAlignVertical="top"
                value={note}
                onChangeText={setNote}
                returnKeyType="default"
                blurOnSubmit={false}
              />
            ) : (
              <VoiceRecorder onEnqueueClip={enqueueAudioClip} />
            )}
          </ScrollView>

          {isTooLong && captureMode === 'text' ? (
            <Text style={styles.limitError}>Too long - try splitting</Text>
          ) : null}

          <View
            style={styles.actionRow}
            onTouchStart={handleVoiceSwipeStart}
            onTouchEnd={handleVoiceSwipeEnd}
          >
            <TouchableOpacity
              style={[
                styles.modeButton,
                captureMode === 'voice' && styles.modeButtonActive,
              ]}
              onPress={() =>
                setCaptureMode((m) => (m === 'voice' ? 'text' : 'voice'))
              }
              accessibilityRole="button"
              accessibilityLabel="Toggle capture mode"
            >
              <Text style={styles.modeButtonText}>
                {captureMode === 'voice' ? 'Text' : 'Voice'}
              </Text>
            </TouchableOpacity>
            {captureMode === 'text' ? (
              <TouchableOpacity
                style={[styles.sendButton, !canSend && styles.sendButtonDisabled]}
                onPress={handleSend}
                disabled={!canSend}
              >
                <Text style={styles.sendButtonText}>Send</Text>
              </TouchableOpacity>
            ) : null}
          </View>

          {outboxStatusRows.length > 0 ? (
            <View style={styles.outboxPanel}>
              <Text style={styles.outboxTitle}>Recent sends</Text>
              {outboxStatusRows.map((row) => (
                <View key={row.id} style={styles.outboxRow}>
                  <Text style={styles.outboxPreview} numberOfLines={1}>
                    {row.preview_text ?? "—"}
                  </Text>
                  <View style={styles.outboxMeta}>
                    <Text style={styles.outboxTime}>
                      {relativeTime(row.created_at)}
                    </Text>
                    <Text
                      style={[
                        styles.outboxStatus,
                        styles[`outboxStatus_${row.state}`],
                      ]}
                    >
                      {statusIcon(row.state)} {statusText(row.state)}
                    </Text>
                  </View>
                </View>
              ))}
            </View>
          ) : null}
        </View>
      </TouchableWithoutFeedback>
    </KeyboardAvoidingView>
    </CaptureErrorBoundary>
  );
}

const styles = StyleSheet.create({
  flex: {
    flex: 1,
  },
  container: {
    flex: 1,
    padding: 20,
    gap: 16,
    backgroundColor: '#fff',
  },
  guide: {
    borderWidth: 1,
    borderColor: '#d6d9de',
    borderRadius: 8,
    padding: 16,
    gap: 12,
    backgroundColor: '#f7f8fa',
  },
  guideText: {
    fontSize: 15,
    lineHeight: 21,
    color: '#222',
  },
  guideButton: {
    alignSelf: 'flex-start',
    borderRadius: 6,
    backgroundColor: '#1f6feb',
    paddingHorizontal: 14,
    paddingVertical: 8,
  },
  guideButtonText: {
    color: '#fff',
    fontWeight: '600',
  },
  inputScroll: {
    flex: 1,
  },
  inputScrollContent: {
    flexGrow: 1,
    gap: 12,
  },
  statusRow: {
    minHeight: 26,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  statusDot: {
    width: 8,
    height: 8,
    borderRadius: 4,
  },
  statusText: {
    fontSize: 13,
    fontWeight: "600",
    color: "#111827",
  },
  statusPeer: {
    flex: 1,
    textAlign: "right",
    fontSize: 12,
    color: "#6b7280",
  },
  input: {
    flex: 1,
    minHeight: 120,
    borderWidth: 1,
    borderColor: '#ccd1d8',
    borderRadius: 8,
    padding: 14,
    fontSize: 16,
  },
  limitError: {
    marginTop: -8,
    fontSize: 13,
    color: "#b91c1c",
  },
  sendButton: {
    flex: 1,
    alignItems: 'center',
    borderRadius: 6,
    backgroundColor: '#1f6feb',
    paddingVertical: 12,
  },
  sendButtonDisabled: {
    opacity: 0.4,
  },
  sendButtonText: {
    color: '#fff',
    fontSize: 16,
    fontWeight: '600',
  },
  actionRow: {
    minHeight: 50,
    flexDirection: 'row',
    alignItems: 'stretch',
    gap: 10,
  },
  modeButton: {
    minWidth: 76,
    alignItems: 'center',
    justifyContent: 'center',
    borderWidth: 1,
    borderColor: '#ccd1d8',
    borderRadius: 6,
    backgroundColor: '#fff',
    paddingHorizontal: 12,
  },
  modeButtonActive: {
    borderColor: '#1f6feb',
    backgroundColor: '#eff6ff',
  },
  modeButtonText: {
    fontSize: 14,
    fontWeight: '600',
    color: '#111827',
  },
  outboxPanel: {
    borderTopWidth: 1,
    borderTopColor: '#e5e7eb',
    paddingTop: 12,
    gap: 8,
  },
  outboxTitle: {
    fontSize: 13,
    fontWeight: '600',
    color: '#374151',
  },
  outboxRow: {
    minHeight: 44,
    justifyContent: "center",
    gap: 2,
  },
  outboxPreview: {
    fontSize: 14,
    color: "#111827",
  },
  outboxMeta: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  outboxTime: {
    fontSize: 12,
    color: "#9ca3af",
  },
  outboxStatus: {
    fontSize: 12,
    fontWeight: "600",
  },
  outboxStatus_done: {
    color: "#166534",
  },
  outboxStatus_sending: {
    color: "#1d4ed8",
  },
  outboxStatus_pending: {
    color: "#92400e",
  },
  outboxStatus_failed: {
    color: "#b91c1c",
  },
  unpairedContainer: {
    flex: 1,
    backgroundColor: '#fff',
  },
  unpairedHint: {
    textAlign: "center",
    fontSize: 14,
    color: "#6b7280",
    paddingTop: 60,
    paddingBottom: 12,
    fontWeight: "500",
  },
});
