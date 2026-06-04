import { useCallback, useEffect, useState } from 'react';
import * as Crypto from 'expo-crypto';
import {
  Alert,
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

import { PairingScanner } from '@/src/pairing/scanner';
import { useAppStore } from '@/src/store';
import { createCaptureTextPayload, encryptCaptureText } from '@/src/crypto/bundle';
import { getRestoredPairingState } from '@/src/spine/session';
import {
  enqueueOutboxItem,
  flushOutbox,
  getRecentOutboxStatuses,
  type OutboxState,
  type OutboxStatusRow,
  QueueFullError,
  subscribeToOutboxChanges,
} from '@/src/outbox/service';

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
  const showFirstCaptureGuide = useAppStore((state) => state.showFirstCaptureGuide);
  const dismissFirstCaptureGuide = useAppStore(
    (state) => state.dismissFirstCaptureGuide,
  );
  const [note, setNote] = useState('');
  const [outboxStatusRows, setOutboxStatusRows] = useState<OutboxStatusRow[]>([]);

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

  if (!isPaired) {
    return (
      <View style={styles.unpairedContainer}>
        <Text style={styles.unpairedHint}>
          Pair with a desktop to start capturing
        </Text>
        <PairingScanner />
      </View>
    );
  }

  const handleSend = async () => {
    const trimmed = note.trim();
    if (!trimmed) return;

    const state = getRestoredPairingState();
    if (!state) return;

    const payload = createCaptureTextPayload({
      id: Crypto.randomUUID(),
      text: trimmed,
      source: "mobile",
      client_ts: new Date().toISOString(),
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

  return (
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
            keyboardShouldPersistTaps="handled"
          >
            <TextInput
              multiline
              placeholder="Capture a note"
              style={styles.input}
              textAlignVertical="top"
              value={note}
              onChangeText={setNote}
              returnKeyType="default"
              blurOnSubmit={false}
            />
          </ScrollView>

          <TouchableOpacity
            style={[styles.sendButton, !note.trim() && styles.sendButtonDisabled]}
            onPress={handleSend}
            disabled={!note.trim()}
          >
            <Text style={styles.sendButtonText}>Send</Text>
          </TouchableOpacity>

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
  sendButton: {
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
