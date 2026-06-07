// Mock expo-task-manager
jest.mock("expo-task-manager", () => {
  const tasks = new Map<string, () => Promise<unknown>>();
  return {
    __esModule: true,
    defineTask: jest.fn((name: string, task: () => Promise<unknown>) => {
      tasks.set(name, task);
    }),
    isTaskDefined: jest.fn((name: string) => tasks.has(name)),
    __runTaskForTests: jest.fn(async (name: string) => {
      const task = tasks.get(name);
      if (!task) {
        throw new Error(`Task not defined: ${name}`);
      }
      return task();
    }),
    __clearTasksForTests: jest.fn(() => {
      tasks.clear();
    }),
  };
});

// Mock expo-background-fetch
jest.mock("expo-background-fetch", () => ({
  __esModule: true,
  BackgroundFetchStatus: { Denied: 1, Restricted: 2, Available: 3 },
  BackgroundFetchResult: { NoData: 1, NewData: 2, Failed: 3 },
  getStatusAsync: jest.fn(async () => 3),
  registerTaskAsync: jest.fn(async () => {}),
  unregisterTaskAsync: jest.fn(async () => {}),
}));

// Mock expo-audio
jest.mock("expo-audio", () => {
  const recorder = {
    uri: "file:///tmp/syncmind-test.m4a",
    prepareToRecordAsync: jest.fn(async () => {}),
    record: jest.fn(),
    stop: jest.fn(async () => {}),
    getStatus: jest.fn(() => ({
      canRecord: true,
      isRecording: false,
      durationMillis: 0,
      mediaServicesDidReset: false,
      metering: -40,
      url: "file:///tmp/syncmind-test.m4a",
    })),
  };

  return {
    __esModule: true,
    requestRecordingPermissionsAsync: jest.fn(async () => ({ granted: true })),
    setAudioModeAsync: jest.fn(async () => {}),
    useAudioRecorder: jest.fn(() => recorder),
    useAudioRecorderState: jest.fn(() => recorder.getStatus()),
    RecordingPresets: {
      HIGH_QUALITY: {},
      LOW_QUALITY: {},
    },
  };
});

// Mock expo-file-system
jest.mock("expo-file-system", () => {
  class File {
    uri: string;

    constructor(uri: string) {
      this.uri = uri;
    }

    async bytes(): Promise<Uint8Array> {
      return new Uint8Array([1, 2, 3, 4]);
    }

    async base64(): Promise<string> {
      return "AQIDBA==";
    }

    info(): { exists: boolean; size: number } {
      return { exists: true, size: 4 };
    }

    delete(): void {}
  }

  return { __esModule: true, File };
});

// Mock expo-image-picker
jest.mock("expo-image-picker", () => ({
  __esModule: true,
  requestCameraPermissionsAsync: jest.fn(async () => ({ granted: true })),
  requestMediaLibraryPermissionsAsync: jest.fn(async () => ({ granted: true })),
  launchCameraAsync: jest.fn(async () => ({ canceled: true, assets: null })),
  launchImageLibraryAsync: jest.fn(async () => ({ canceled: true, assets: null })),
}));

// Mock expo-image-manipulator
jest.mock("expo-image-manipulator", () => ({
  __esModule: true,
  SaveFormat: {
    JPEG: "jpeg",
    PNG: "png",
    WEBP: "webp",
  },
  manipulateAsync: jest.fn(async () => ({
    uri: "file:///tmp/syncmind-processed.jpg",
    width: 1,
    height: 1,
    base64: "AQIDBA==",
  })),
}));
