## SDK 56 Audio And File APIs

Checked Expo SDK 56 versioned docs before implementation.

- Audio recording package: `expo-audio`
- Install command: `npx expo install expo-audio`
- Recording API shape: `useAudioRecorder(options, statusListener)`, `useAudioRecorderState(recorder)`, `AudioModule.requestRecordingPermissionsAsync()`, and `setAudioModeAsync({ allowsRecording: true, playsInSilentMode: true })`
- Recording options: `RecordingOptions` supports `extension`, `sampleRate`, `numberOfChannels`, `bitRate`, Android `outputFormat` / `audioEncoder`, iOS `outputFormat`, and `isMeteringEnabled`
- File package: `expo-file-system`
- Install command: `npx expo install expo-file-system`
- File API shape: `new File(uri).base64()`, `new File(uri).bytes()`, `new File(uri).delete()`, and `new File(uri).info()`

Sources:
- https://docs.expo.dev/versions/v56.0.0/sdk/audio/
- https://docs.expo.dev/versions/v56.0.0/sdk/filesystem/
