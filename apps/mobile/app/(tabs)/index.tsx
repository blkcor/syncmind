import { useState } from 'react';
import {
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

export default function CaptureScreen() {
  const isPaired = useAppStore((state) => state.isPaired);
  const showFirstCaptureGuide = useAppStore((state) => state.showFirstCaptureGuide);
  const dismissFirstCaptureGuide = useAppStore(
    (state) => state.dismissFirstCaptureGuide,
  );
  const [note, setNote] = useState('');

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

  const handleSend = () => {
    const trimmed = note.trim();
    if (!trimmed) return;
    // TODO: wire to spine_send_note
    setNote('');
    Keyboard.dismiss();
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
