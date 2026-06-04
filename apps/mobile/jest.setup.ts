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
