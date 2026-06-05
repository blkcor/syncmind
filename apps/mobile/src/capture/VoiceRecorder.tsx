import { useCallback, useEffect, useRef, useState } from 'react';
import {
  Alert,
  AppState,
  type AccessibilityActionEvent,
  StyleSheet,
  Text,
  TouchableOpacity,
  View,
} from 'react-native';
import { useAudioRecorder, useAudioRecorderState, AudioModule, setAudioModeAsync } from 'expo-audio';
import { File as ExpoFile } from 'expo-file-system';

import {
  AUDIO_RECORDING_OPTIONS,
  createAudioCaptureSession,
  type AudioCaptureReadResult,
  type AudioCaptureStartResult,
} from './audio';

const BACKGROUND_INTERRUPT_MS = 30_000;

type VoiceStatus = 'idle' | 'setting-up' | 'recording' | 'stopping' | 'enqueuing' | 'error';

interface VoiceRecorderProps {
  onEnqueueClip: (clip: AudioCaptureReadResult) => Promise<void>;
}

export function VoiceRecorder({ onEnqueueClip }: VoiceRecorderProps) {
  const [voiceStatus, setVoiceStatus] = useState<VoiceStatus>('setting-up');
  const [interruptedClip, setInterruptedClip] =
    useState<AudioCaptureReadResult | null>(null);
  const [audioMetering, setAudioMetering] = useState(-40);
  const backgroundInterruptTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isStartingRef = useRef(false);

  const audioRecorder = useAudioRecorder(AUDIO_RECORDING_OPTIONS);
  const recorderState = useAudioRecorderState(audioRecorder, 200);
  const isVoiceRecording = voiceStatus === 'recording';

  // Configure the global audio session once on mount. Recording must not start
  // until this settles — calling prepareToRecordAsync before the session is
  // configured can trigger a native SIGABRT in expo-audio on iOS.
  useEffect(() => {
    let cancelled = false;
    setAudioModeAsync({
      allowsRecording: true,
      playsInSilentMode: true,
    })
      .then(() => {
        if (!cancelled) setVoiceStatus('idle');
      })
      .catch((err) => {
        console.error('setAudioModeAsync failed:', err);
        if (!cancelled) setVoiceStatus('error');
      });
    return () => { cancelled = true; };
  }, []);

  const createVoiceSession = useCallback(() => {
    return createAudioCaptureSession({
      recorder: audioRecorder,
      requestPermissions: () =>
        AudioModule.requestRecordingPermissionsAsync(),
      fileFromUri: (uri) => new ExpoFile(uri),
    });
  }, [audioRecorder]);

  const discardInterruptedClip = useCallback(() => {
    setInterruptedClip(null);
  }, []);

  const keepInterruptedClip = useCallback(
    async (clip = interruptedClip) => {
      if (!clip) return;
      setInterruptedClip(null);
      await onEnqueueClip(clip);
    },
    [onEnqueueClip, interruptedClip],
  );

  const handleInterruptedRecording = useCallback(async () => {
    if (voiceStatus !== 'recording') return;
    setVoiceStatus('stopping');
    try {
      const session = createVoiceSession();
      const clip = await session.stopAndRead();
      setInterruptedClip(clip);
      setVoiceStatus('idle');
      Alert.alert('Recording Interrupted', 'Keep this audio capture?', [
        { text: 'Discard', style: 'destructive', onPress: discardInterruptedClip },
        { text: 'Keep', onPress: () => { void keepInterruptedClip(clip); } },
      ]);
    } catch (err) {
      console.error('Failed to handle interrupted recording:', err);
      setVoiceStatus('error');
    }
  }, [createVoiceSession, discardInterruptedClip, keepInterruptedClip, voiceStatus]);

  const handleVoicePressIn = useCallback(async () => {
    // Prevent re-entry (onPressIn can fire rapidly on some devices).
    if (isStartingRef.current) return;
    if (voiceStatus !== 'idle') return;

    isStartingRef.current = true;
    const session = createVoiceSession();
    try {
      const result: AudioCaptureStartResult = await session.start();
      if (result.status === 'permission-denied') {
        Alert.alert(
          'Microphone Permission',
          'Enable microphone access in system settings.',
        );
        setVoiceStatus('idle');
        return;
      }
      setAudioMetering(result.metering ?? -40);
      setVoiceStatus('recording');
    } catch (err) {
      console.error('Failed to start recording:', err);
      setVoiceStatus('error');
      Alert.alert('Recording Failed', 'Could not start audio capture.');
    } finally {
      isStartingRef.current = false;
    }
  }, [createVoiceSession, voiceStatus]);

  const stopAndEnqueueVoice = useCallback(async () => {
    if (voiceStatus !== 'recording') return;
    setVoiceStatus('stopping');
    try {
      const session = createVoiceSession();
      const clip = await session.stopAndRead();
      setVoiceStatus('enqueuing');
      await onEnqueueClip(clip);
      setVoiceStatus('idle');
    } catch (err) {
      console.error('Failed to stop recording:', err);
      setVoiceStatus('error');
      Alert.alert('Recording Error', 'Could not read audio capture.');
    }
  }, [createVoiceSession, onEnqueueClip, voiceStatus]);

  const handleVoicePressOut = useCallback(async () => {
    await stopAndEnqueueVoice();
  }, [stopAndEnqueueVoice]);

  const handleVoiceToggleRecording = useCallback(async () => {
    if (voiceStatus !== 'idle' && voiceStatus !== 'recording') return;
    if (voiceStatus === 'recording') {
      await stopAndEnqueueVoice();
      return;
    }
    await handleVoicePressIn();
  }, [handleVoicePressIn, stopAndEnqueueVoice, voiceStatus]);

  useEffect(() => {
    if (typeof recorderState.metering === 'number') {
      setAudioMetering(recorderState.metering);
    }
  }, [recorderState.metering]);

  const handleVoiceAccessibilityAction = (event: AccessibilityActionEvent) => {
    if (event.nativeEvent.actionName === 'activate') {
      void handleVoiceToggleRecording();
    }
  };

  useEffect(() => {
    const subscription = AppState.addEventListener('change', (nextState) => {
      if (nextState === 'background' && voiceStatus === 'recording') {
        backgroundInterruptTimer.current = setTimeout(() => {
          void handleInterruptedRecording();
        }, BACKGROUND_INTERRUPT_MS);
        return;
      }

      if (nextState === 'active' && backgroundInterruptTimer.current) {
        clearTimeout(backgroundInterruptTimer.current);
        backgroundInterruptTimer.current = null;
      }
    });

    return () => {
      if (backgroundInterruptTimer.current) {
        clearTimeout(backgroundInterruptTimer.current);
        backgroundInterruptTimer.current = null;
      }
      subscription.remove();
    };
  }, [voiceStatus, handleInterruptedRecording]);

  return (
    <View style={styles.container}>
      <View style={styles.voicePanel}>
        <View style={styles.waveform} accessibilityLabel="Recording level">
          {[0, 1, 2, 3, 4].map((bar) => (
            <View
              key={bar}
              style={[
                styles.waveformBar,
                {
                  height: Math.max(8, 18 + audioMetering + bar * 6),
                },
              ]}
            />
          ))}
        </View>
        <Text style={styles.voiceStatus}>
          {voiceStatus === 'setting-up'
            ? 'Setting up audio...'
            : isVoiceRecording
              ? 'Recording'
              : voiceStatus === 'error'
                ? 'Audio unavailable'
                : 'Hold to record'}
        </Text>
      </View>

      <TouchableOpacity
        style={[
          styles.recordButton,
          isVoiceRecording && styles.recordButtonActive,
          voiceStatus !== 'idle' && voiceStatus !== 'recording' && styles.recordButtonDisabled,
        ]}
        onPressIn={handleVoicePressIn}
        onPressOut={handleVoicePressOut}
        disabled={voiceStatus !== 'idle' && voiceStatus !== 'recording'}
        accessibilityRole="button"
        accessibilityLabel="Record audio capture"
        accessibilityHint="Hold to record, release to send"
        accessibilityActions={[{ name: 'activate', label: 'Toggle recording' }]}
        onAccessibilityAction={handleVoiceAccessibilityAction}
      >
        <Text style={styles.recordButtonText}>
          {voiceStatus === 'setting-up'
            ? 'Wait...'
            : isVoiceRecording
              ? 'Release'
              : 'Hold'}
        </Text>
      </TouchableOpacity>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    gap: 18,
  },
  voicePanel: {
    flex: 1,
    minHeight: 180,
    alignItems: 'center',
    justifyContent: 'center',
    gap: 18,
    borderWidth: 1,
    borderColor: '#ccd1d8',
    borderRadius: 8,
    padding: 18,
  },
  waveform: {
    height: 72,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  waveformBar: {
    width: 8,
    borderRadius: 4,
    backgroundColor: '#1f6feb',
  },
  voiceStatus: {
    fontSize: 15,
    fontWeight: '600',
    color: '#374151',
  },
  recordButton: {
    minHeight: 50,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 25,
    backgroundColor: '#dc2626',
  },
  recordButtonActive: {
    backgroundColor: '#991b1b',
  },
  recordButtonDisabled: {
    opacity: 0.4,
  },
  recordButtonText: {
    color: '#fff',
    fontSize: 16,
    fontWeight: '700',
  },
});
