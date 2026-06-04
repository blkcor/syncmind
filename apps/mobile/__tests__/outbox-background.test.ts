/* eslint-disable @typescript-eslint/no-require-imports */

jest.mock("../src/outbox/service", () => ({
  initOutbox: jest.fn(async () => {}),
  flushOutbox: jest.fn(async () => ({ attemptedUploads: 1 })),
  getPendingCount: jest.fn(async () => 0),
}));

describe("outbox background flush task", () => {
  beforeEach(() => {
    jest.resetModules();
  });

  it("returns NewData when flush attempts an upload that drains the queue", async () => {
    let runTask: (name: string) => Promise<number>;

    jest.isolateModules(() => {
      require("../src/outbox/background");
      const TaskManager = require("expo-task-manager");
      runTask = TaskManager.__runTaskForTests;
    });

    const result = await runTask!("SYNCMIND_OUTBOX_FLUSH");

    const BackgroundFetch = require("expo-background-fetch");
    expect(result).toBe(BackgroundFetch.BackgroundFetchResult.NewData);
  });
});
