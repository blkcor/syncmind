import * as BackgroundFetch from "expo-background-fetch";
import * as TaskManager from "expo-task-manager";
import { initOutbox, flushOutbox } from "./service";

const TASK_NAME = "SYNCMIND_OUTBOX_FLUSH";

TaskManager.defineTask(TASK_NAME, async () => {
  try {
    await initOutbox();
    const result = await flushOutbox();

    if (result.attemptedUploads > 0) {
      return BackgroundFetch.BackgroundFetchResult.NewData;
    }
    return BackgroundFetch.BackgroundFetchResult.NoData;
  } catch {
    return BackgroundFetch.BackgroundFetchResult.Failed;
  }
});

export async function registerBackgroundFlush(): Promise<void> {
  try {
    const status = await BackgroundFetch.getStatusAsync();
    if (status === BackgroundFetch.BackgroundFetchStatus.Denied) {
      return;
    }

    if (!TaskManager.isTaskDefined(TASK_NAME)) {
      return;
    }

    await BackgroundFetch.registerTaskAsync(TASK_NAME, {
      minimumInterval: 60 * 15,
      stopOnTerminate: false,
      startOnBoot: true,
    });
  } catch {
    // Background fetch registration is opportunistic; failure is non-fatal.
  }
}

export async function unregisterBackgroundFlush(): Promise<void> {
  try {
    if (TaskManager.isTaskDefined(TASK_NAME)) {
      await BackgroundFetch.unregisterTaskAsync(TASK_NAME);
    }
  } catch {
    // Best-effort cleanup
  }
}
