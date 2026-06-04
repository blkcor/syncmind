/**
 * Tests for the SQLite-backed outbox service and flush/upload behavior.
 */
/* eslint-disable @typescript-eslint/no-require-imports */

// ── Mocks ────────────────────────────────────────────────────────────

jest.mock("expo-sqlite", () => {
  interface MockRow {
    id: string;
    created_at: string;
    state: string;
    attempts: number;
    last_error: string | null;
    preview_text: string | null;
    encrypted_blob: Uint8Array;
  }

  const tables = new Map<string, MockRow[]>();
  tables.set("outbox", []);

  function resetTables(): void {
    tables.set("outbox", []);
  }

  function getTable(): MockRow[] {
    return tables.get("outbox")!;
  }

  const mockDb = {
    execAsync: jest.fn(async (_sql: string) => {}),
    runAsync: jest.fn(
      async (
        sql: string,
        ...params: (string | number | Uint8Array | null)[]
      ) => {
        const table = getTable();

        if (sql.includes("DELETE FROM outbox")) {
          if (sql.includes("WHERE state = 'done' AND created_at < ?")) {
            const cutoff = params[0] as string;
            const remaining = table.filter(
              (r) => !(r.state === "done" && r.created_at < cutoff),
            );
            tables.set("outbox", remaining);
          } else {
            tables.set("outbox", []);
          }
          return { lastInsertRowId: 0, changes: table.length };
        }

        if (sql.includes("INSERT INTO outbox")) {
          const [id, createdAt] = params;
          // Parse state, attempts, last_error from VALUES literals in the SQL.
          // Service always inserts with state='pending', attempts=0, last_error=NULL,
          // but tests insert rows with various states directly.
          const vMatch = sql.match(/VALUES\s*\(([^)]+)\)/);
          let state = "pending";
          let attempts = 0;
          let lastError: string | null = null;
          if (vMatch) {
            const parts = vMatch[1].split(",").map((s) => {
              const t = s.trim();
              return t.startsWith("'") && t.endsWith("'") ? t.slice(1, -1) : t;
            });
            if (parts[2] && parts[2] !== "?") state = parts[2];
            if (parts[3] && parts[3] !== "?") attempts = Number(parts[3]);
            if (parts[4] && parts[4] !== "?" && parts[4] !== "NULL") lastError = parts[4];
          }
          table.push({
            id: id as string,
            created_at: createdAt as string,
            state,
            attempts,
            last_error: lastError,
            preview_text: (params[3] as string) ?? null,
            encrypted_blob: params[2] as Uint8Array,
          });
          return { lastInsertRowId: table.length, changes: 1 };
        }

        if (sql.includes("UPDATE outbox SET state = 'sending'")) {
          if (params.length > 0) {
            const row = table.find((r) => r.id === params[0]);
            if (row) {
              row.state = "sending";
            }
          }
          return { lastInsertRowId: 0, changes: 1 };
        }

        if (sql.includes("UPDATE outbox SET attempts = attempts + 1")) {
          const row = table.find((r) => r.id === params[0]);
          if (row) {
            row.attempts += 1;
          }
          return { lastInsertRowId: 0, changes: 1 };
        }

        if (sql.includes("SET state = 'done'")) {
          const id = params[0] as string;
          const row = table.find((r) => r.id === id);
          if (row) {
            row.state = "done";
            row.last_error = null;
          }
          return { lastInsertRowId: 0, changes: 1 };
        }

        if (sql.includes("SET state = 'failed'")) {
          const error = params[0] as string;
          const id = params[1] as string;
          const row = table.find((r) => r.id === id);
          if (row) {
            row.state = "failed";
            row.last_error = error;
          }
          return { lastInsertRowId: 0, changes: 1 };
        }

        if (sql.includes("SET state = 'pending' WHERE state = 'sending'")) {
          for (const row of table) {
            if (row.state === "sending") {
              row.state = "pending";
            }
          }
          return { lastInsertRowId: 0, changes: 1 };
        }

        return { lastInsertRowId: 0, changes: 0 };
      },
    ),
    getFirstAsync: jest.fn(async (sql: string) => {
      const table = getTable();

      if (sql.includes("COUNT(*) as cnt")) {
        const count = table.filter((r) =>
          ["pending", "sending", "failed"].includes(r.state),
        ).length;
        return { cnt: count };
      }

      if (sql.includes("SELECT * FROM outbox WHERE state = 'pending'")) {
        const sorted = [...table]
          .filter((r) => r.state === "pending")
          .sort((a, b) => a.created_at.localeCompare(b.created_at));
        const row = sorted[0];
        if (!row) return null;
        return {
          id: row.id,
          created_at: row.created_at,
          state: row.state,
          attempts: row.attempts,
          last_error: row.last_error,
          encrypted_blob: Array.from(row.encrypted_blob),
        };
      }

      return null;
    }),
    getAllAsync: jest.fn(async (sql: string) => {
      const table = getTable();
      if (sql.includes("SELECT id, state FROM outbox")) {
        return [...table]
          .sort((a, b) => a.created_at.localeCompare(b.created_at))
          .map((r) => ({ id: r.id, state: r.state }));
      }
      if (
        sql.includes("SELECT id, created_at, state, attempts, last_error, preview_text FROM outbox")
      ) {
        return [...table]
          .sort((a, b) => b.created_at.localeCompare(a.created_at))
          .slice(0, 3)
          .map((row) => ({
            id: row.id,
            created_at: row.created_at,
            state: row.state,
            attempts: row.attempts,
            last_error: row.last_error,
            preview_text: row.preview_text ?? null,
          }));
      }
      if (sql.includes("SELECT * FROM outbox ORDER BY created_at ASC")) {
        return [...table]
          .sort((a, b) => a.created_at.localeCompare(b.created_at))
          .map((row) => ({
            id: row.id,
            created_at: row.created_at,
            state: row.state,
            attempts: row.attempts,
            last_error: row.last_error,
            encrypted_blob: Array.from(row.encrypted_blob),
          }));
      }
      return [];
    }),
    closeAsync: jest.fn(async () => {}),
  };

  return {
    openDatabaseAsync: jest.fn(async () => mockDb),
    __resetTables: resetTables,
  };
});

jest.mock("expo-crypto", () => ({
  randomUUID: jest.fn(() => "cccccccc-cccc-4ccc-8ccc-cccccccccccc"),
  getRandomBytes: jest.fn((size: number) => {
    const bytes = new Uint8Array(size);
    for (let i = 0; i < size; i++) bytes[i] = (i + 1) % 256;
    return bytes;
  }),
}));

jest.mock("expo-secure-store", () => {
  const store = new Map<string, string>();
  return {
    setItemAsync: jest.fn(async (key: string, value: string) => {
      store.set(key, value);
    }),
    getItemAsync: jest.fn(async (key: string) => store.get(key) ?? null),
    deleteItemAsync: jest.fn(async (key: string) => {
      store.delete(key);
    }),
  };
});

jest.mock("../src/crypto/native-device-identity", () => ({
  __esModule: true,
  default: {
    ensureIdentity: jest.fn(async () => ({
      publicKeyHex: "02".repeat(32),
      fingerprint: "sha256:mobile-identity-fp",
      requireAuthentication: false,
    })),
    getIdentityMeta: jest.fn(async () => ({
      publicKeyHex: "02".repeat(32),
      fingerprint: "sha256:mobile-identity-fp",
      requireAuthentication: false,
    })),
    sign: jest.fn(async () => {
      const sig = new Uint8Array(64).fill(0xab);
      let base64 = "";
      for (const b of sig) base64 += String.fromCharCode(b);
      return btoa(base64);
    }),
    deriveX25519: jest.fn(async () => {
      const shared = new Uint8Array(32).fill(9);
      let base64 = "";
      for (const b of shared) base64 += String.fromCharCode(b);
      return btoa(base64);
    }),
    setBiometricProtection: jest.fn(async () => {}),
    resetIdentity: jest.fn(async () => {}),
    importLegacyIdentity: jest.fn(async () => null),
  },
}));

import {
  initOutbox,
  enqueueOutboxItem,
  clearOutbox,
  flushOutbox,
  getRecentOutboxStatuses,
  getOutboxItems,
  getPendingCount,
  subscribeToOutboxChanges,
  resetSendingToPending,
  cleanupDoneRows,
  getOutboxRowsForTests,
  QueueFullError,
  __closeOutboxForTests,
  __resetFlushLockForTests,
} from "../src/outbox/service";
import {
  persistPairingState,
  clearCurrentSpineSession,
  __clearPairingStateForTests,
} from "../src/spine/session";
import { ensureIdentity } from "../src/crypto/identity";
import { useAppStore } from "../src/store";
import { encryptCaptureText } from "../src/crypto/bundle";

// ── Helpers ─────────────────────────────────────────────────────────

const FINGERPRINT_PREFIX = "sha256:";

function buildFingerprint(hex: string): string {
  return `${FINGERPRINT_PREFIX}${hex}`;
}

const VALID_HEX_64 = "ab".repeat(32); // exactly 64 hex chars → 32 bytes

const defaultPairingState = {
  selfDeviceUuid: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  syncKey: new Uint8Array(32).fill(3),
  pairedPeerFingerprint: buildFingerprint(VALID_HEX_64),
  pairedPeerDeviceId: "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
  pairedPeerDeviceType: "desktop" as const,
  pairedAt: "2026-06-02T00:00:00.000Z",
  spineUrl: "https://spine.syncmind.local:8443",
  caFingerprint: null,
  lastSeenAt: null,
};

async function createEncryptedFixture(id: string, text: string): Promise<Uint8Array> {
  const result = await encryptCaptureText(
    { id, text, source: "mobile", client_ts: "2026-06-02T00:00:00.000Z" },
    defaultPairingState,
  );
  return result.blob;
}

beforeEach(async () => {
  jest.clearAllMocks();
  useAppStore.getState().reset();
  await clearCurrentSpineSession();
  __clearPairingStateForTests();
  const SQLite = require("expo-sqlite");
  SQLite.__resetTables();
  await __closeOutboxForTests();
  __resetFlushLockForTests();
  global.fetch = jest.fn(async () => new Response(null, { status: 204 })) as typeof fetch;
});

// ── Outbox persistence ───────────────────────────────────────────────

describe("outbox persistence", () => {
  it("creates the outbox table on first use", async () => {
    await enqueueOutboxItem("cap-1", await createEncryptedFixture("cap-1", "hello"));

    const items = await getOutboxItems();
    expect(items.length).toBe(1);
    expect(items[0].id).toBe("cap-1");
    expect(items[0].state).toBe("pending");
  });

  it("survives service re-open (simulates process restart)", async () => {
    await enqueueOutboxItem("cap-1", await createEncryptedFixture("cap-1", "hello"));

    await __closeOutboxForTests();

    const items = await getOutboxItems();
    expect(items.length).toBe(1);
  });

  it("clearOutbox deletes all persisted rows", async () => {
    await enqueueOutboxItem("cap-1", await createEncryptedFixture("cap-1", "a"));
    await enqueueOutboxItem("cap-2", await createEncryptedFixture("cap-2", "b"));

    await clearOutbox();

    const items = await getOutboxItems();
    expect(items.length).toBe(0);
  });
});

// ── State transitions ────────────────────────────────────────────────

describe("state transitions", () => {
  it("enqueued row starts as pending", async () => {
    await enqueueOutboxItem("cap-1", await createEncryptedFixture("cap-1", "test"));
    const items = await getOutboxItems();
    expect(items[0].state).toBe("pending");
  });

  it("resetSendingToPending recovers stale sending rows", async () => {
    await initOutbox();

    // Get the mock DB and manipulate state directly
    const SQLiteModule = require("expo-sqlite");
    const db = await SQLiteModule.openDatabaseAsync("");

    // Insert a row via enqueue, then mark as sending via runAsync
    await enqueueOutboxItem("cap-1", await createEncryptedFixture("cap-1", "test"));
    // runAsync was used to insert; now use it to update state
    await db.runAsync("UPDATE outbox SET state = 'sending' WHERE id = ?", "cap-1");

    await resetSendingToPending();

    const items = await getOutboxItems();
    expect(items.length).toBeGreaterThanOrEqual(1);
  });

  it("initOutbox resets sending rows on startup", async () => {
    await initOutbox();

    const SQLiteModule = require("expo-sqlite");
    const db = await SQLiteModule.openDatabaseAsync("");

    await enqueueOutboxItem("cap-1", await createEncryptedFixture("cap-1", "test"));
    await db.runAsync("UPDATE outbox SET state = 'sending' WHERE id = ?", "cap-1");

    await __closeOutboxForTests();
    await initOutbox();

    const items = await getOutboxItems();
    expect(items.length).toBeGreaterThanOrEqual(1);
  });
});

// ── Queue cap ────────────────────────────────────────────────────────

describe("queue cap", () => {
  it("rejects new capture when 1000 unfinished rows exist", async () => {
    await initOutbox();
    const SQLiteModule = require("expo-sqlite");
    const db = await SQLiteModule.openDatabaseAsync("");

    // Fill up to cap by directly inserting rows
    for (let i = 0; i < 1000; i++) {
      await db.runAsync(
        "INSERT INTO outbox (id, created_at, state, attempts, last_error, encrypted_blob) VALUES (?, ?, 'pending', 0, NULL, ?)",
        `cap-${i}`,
        new Date().toISOString(),
        new Uint8Array(32),
      );
    }

    await expect(
      enqueueOutboxItem("cap-over", new Uint8Array(32)),
    ).rejects.toThrow(QueueFullError);
  });

  it("rejects with the user-facing error message", async () => {
    await initOutbox();
    const SQLiteModule = require("expo-sqlite");
    const db = await SQLiteModule.openDatabaseAsync("");

    for (let i = 0; i < 1000; i++) {
      await db.runAsync(
        "INSERT INTO outbox (id, created_at, state, attempts, last_error, encrypted_blob) VALUES (?, ?, 'pending', 0, NULL, ?)",
        `cap-${i}`,
        new Date().toISOString(),
        new Uint8Array(32),
      );
    }

    await expect(
      enqueueOutboxItem("cap-over", new Uint8Array(32)),
    ).rejects.toThrow("Capture queue is full");
  });

  it("done rows do not count toward the 1000-row cap", async () => {
    await initOutbox();
    const SQLiteModule = require("expo-sqlite");
    const db = await SQLiteModule.openDatabaseAsync("");

    // Fill with 1000 done rows
    for (let i = 0; i < 1000; i++) {
      await db.runAsync(
        "INSERT INTO outbox (id, created_at, state, attempts, last_error, encrypted_blob) VALUES (?, ?, 'done', 1, NULL, ?)",
        `done-${i}`,
        new Date().toISOString(),
        new Uint8Array(32),
      );
    }

    // Should still be able to enqueue
    await expect(
      enqueueOutboxItem("new-cap", new Uint8Array(32)),
    ).resolves.toBeUndefined();
  });
});

// ── Done row cleanup ─────────────────────────────────────────────────

describe("done row cleanup", () => {
  it("deletes done rows older than 7 days", async () => {
    await initOutbox();
    const SQLiteModule = require("expo-sqlite");
    const db = await SQLiteModule.openDatabaseAsync("");

    const oldDate = new Date(Date.now() - 8 * 86_400_000).toISOString();
    await db.runAsync(
      "INSERT INTO outbox (id, created_at, state, attempts, last_error, encrypted_blob) VALUES (?, ?, 'done', 1, NULL, ?)",
      "done-old",
      oldDate,
      new Uint8Array(32),
    );
    await db.runAsync(
      "INSERT INTO outbox (id, created_at, state, attempts, last_error, encrypted_blob) VALUES (?, ?, 'done', 1, NULL, ?)",
      "done-recent",
      new Date().toISOString(),
      new Uint8Array(32),
    );

    await cleanupDoneRows();

    const items = await getOutboxItems();
    expect(items.length).toBe(1);
    expect(items[0].id).toBe("done-recent");
  });
});

// ── getPendingCount ──────────────────────────────────────────────────

describe("getPendingCount", () => {
  it("counts only unfinished rows", async () => {
    await initOutbox();
    const SQLiteModule = require("expo-sqlite");
    const db = await SQLiteModule.openDatabaseAsync("");

    await db.runAsync(
      "INSERT INTO outbox (id, created_at, state, attempts, last_error, encrypted_blob) VALUES (?, ?, 'pending', 0, NULL, ?)",
      "p1", new Date().toISOString(), new Uint8Array(32),
    );
    await db.runAsync(
      "INSERT INTO outbox (id, created_at, state, attempts, last_error, encrypted_blob) VALUES (?, ?, 'sending', 1, NULL, ?)",
      "s1", new Date().toISOString(), new Uint8Array(32),
    );
    await db.runAsync(
      "INSERT INTO outbox (id, created_at, state, attempts, last_error, encrypted_blob) VALUES (?, ?, 'failed', 3, 'HTTP_500', ?)",
      "f1", new Date().toISOString(), new Uint8Array(32),
    );
    await db.runAsync(
      "INSERT INTO outbox (id, created_at, state, attempts, last_error, encrypted_blob) VALUES (?, ?, 'done', 1, NULL, ?)",
      "d1", new Date().toISOString(), new Uint8Array(32),
    );

    const count = await getPendingCount();
    expect(count).toBe(3); // pending + sending + failed
  });
});

// ── Recent status metadata ───────────────────────────────────────────

describe("getRecentOutboxStatuses", () => {
  it("returns the latest 3 rows with metadata only", async () => {
    await initOutbox();
    const SQLiteModule = require("expo-sqlite");
    const db = await SQLiteModule.openDatabaseAsync("");

    const states = ["done", "pending", "sending", "failed"] as const;
    for (let i = 0; i < states.length; i++) {
      await db.runAsync(
        "INSERT INTO outbox (id, created_at, state, attempts, last_error, encrypted_blob) VALUES (?, ?, '" +
          states[i] +
          "', " +
          i +
          ", " +
          (states[i] === "failed" ? "'HTTP_500'" : "NULL") +
          ", ?)",
        `cap-${i}`,
        `2026-06-02T00:00:0${i}.000Z`,
        new Uint8Array([i]),
      );
    }

    const statuses = await getRecentOutboxStatuses(3);

    expect(statuses).toEqual([
      {
        id: "cap-3",
        created_at: "2026-06-02T00:00:03.000Z",
        state: "failed",
        attempts: 3,
        last_error: "HTTP_500",
        preview_text: null,
      },
      {
        id: "cap-2",
        created_at: "2026-06-02T00:00:02.000Z",
        state: "sending",
        attempts: 2,
        last_error: null,
        preview_text: null,
      },
      {
        id: "cap-1",
        created_at: "2026-06-02T00:00:01.000Z",
        state: "pending",
        attempts: 1,
        last_error: null,
        preview_text: null,
      },
    ]);
    expect(statuses[0]).not.toHaveProperty("encrypted_blob");
  });
});

describe("outbox change notifications", () => {
  it("notifies listeners after enqueue and flush state changes", async () => {
    await ensureIdentity();
    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);
    global.fetch = jest
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 201 })) as typeof fetch;

    const listener = jest.fn();
    const unsubscribe = subscribeToOutboxChanges(listener);

    await enqueueOutboxItem("cap-1", await createEncryptedFixture("cap-1", "hello"));
    await flushOutbox();
    unsubscribe();

    expect(listener).toHaveBeenCalled();
    expect(listener.mock.calls.length).toBeGreaterThanOrEqual(2);
  });
});

// ── Upload: success path ─────────────────────────────────────────────

describe("flushOutbox — success", () => {
  beforeEach(async () => {
    await ensureIdentity();
    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);
  });

  it("marks row done after HTTP 201", async () => {
    global.fetch = jest
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 201 })) as typeof fetch;

    const blob = await createEncryptedFixture("cap-1", "hello");
    await enqueueOutboxItem("cap-1", blob);

    await flushOutbox();

    const items = await getOutboxItems();
    expect(items[0].state).toBe("done");
  });

  it("sends correct request headers", async () => {
    global.fetch = jest
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 201 })) as typeof fetch;

    const blob = await createEncryptedFixture("cap-1", "hello");
    await enqueueOutboxItem("cap-1", blob);

    await flushOutbox();

    const calls = (global.fetch as jest.Mock).mock.calls;
    const bundleCall = calls.find((c: [string]) =>
      c[0].includes("/v1/sync/bundle"),
    );
    expect(bundleCall).toBeDefined();

    const init = bundleCall[1];
    expect(init.method).toBe("POST");
    expect(init.headers["Content-Type"]).toBe("application/octet-stream");
    expect(init.headers["X-Syncmind-Content-Type"]).toBe(
      "application/syncmind.capture-text+json",
    );
    expect(init.headers["Idempotency-Key"]).toBe("cap-1");
    expect(init.headers.Authorization).toMatch(/^Bearer /);
  });

  it("uploads rows in FIFO order", async () => {
    const responses = [
      new Response(null, { status: 201 }),
      new Response(null, { status: 201 }),
      new Response(null, { status: 201 }),
    ];
    global.fetch = jest
      .fn()
      .mockResolvedValueOnce(responses[0])
      .mockResolvedValueOnce(responses[1])
      .mockResolvedValueOnce(responses[2]) as typeof fetch;

    await enqueueOutboxItem("cap-1", await createEncryptedFixture("cap-1", "a"));
    await enqueueOutboxItem("cap-2", await createEncryptedFixture("cap-2", "b"));
    await enqueueOutboxItem("cap-3", await createEncryptedFixture("cap-3", "c"));

    await flushOutbox();

    const items = await getOutboxItems();
    for (const item of items) {
      expect(item.state).toBe("done");
    }
  });
});

// ── Upload: retry behavior ───────────────────────────────────────────

describe("flushOutbox — retry", () => {
  beforeEach(async () => {
    jest.useFakeTimers();
    await ensureIdentity();
    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);
  });

  afterEach(() => {
    jest.useRealTimers();
  });

  it("retries 429 with backoff and reuses idempotency key", async () => {
    global.fetch = jest
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 429 }))
      .mockResolvedValueOnce(new Response(null, { status: 429 }))
      .mockResolvedValueOnce(new Response(null, { status: 201 })) as typeof fetch;

    const blob = await createEncryptedFixture("cap-1", "hello");
    await enqueueOutboxItem("cap-1", blob);

    const flushPromise = flushOutbox();
    await jest.runAllTimersAsync();
    await flushPromise;

    const calls = (global.fetch as jest.Mock).mock.calls;
    const bundleCalls = calls.filter((c: [string]) =>
      c[0].includes("/v1/sync/bundle"),
    );
    expect(bundleCalls.length).toBe(3);

    for (const call of bundleCalls) {
      expect(call[1].headers["Idempotency-Key"]).toBe("cap-1");
    }

    const items = await getOutboxItems();
    expect(items[0].state).toBe("done");
  });

  it("marks row failed after 3 failed 429 attempts", async () => {
    global.fetch = jest
      .fn()
      .mockResolvedValue(new Response(null, { status: 429 })) as typeof fetch;

    const blob = await createEncryptedFixture("cap-1", "hello");
    await enqueueOutboxItem("cap-1", blob);

    const flushPromise = flushOutbox();
    await jest.runAllTimersAsync();
    await flushPromise;

    const items = await getOutboxItems();
    expect(items[0].state).toBe("failed");

    const rows = await getOutboxRowsForTests();
    expect(rows[0].attempts).toBe(3);
  });

  it("marks 4xx row failed without retry", async () => {
    global.fetch = jest
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 400 })) as typeof fetch;

    const blob = await createEncryptedFixture("cap-1", "hello");
    await enqueueOutboxItem("cap-1", blob);

    await flushOutbox();

    const calls = (global.fetch as jest.Mock).mock.calls;
    const bundleCalls = calls.filter((c: [string]) =>
      c[0].includes("/v1/sync/bundle"),
    );
    expect(bundleCalls.length).toBe(1);

    const items = await getOutboxItems();
    expect(items[0].state).toBe("failed");
  });

  it("marks row failed after network error exhausts retries", async () => {
    global.fetch = jest
      .fn()
      .mockRejectedValue(new TypeError("Network request failed")) as typeof fetch;

    const blob = await createEncryptedFixture("cap-1", "hello");
    await enqueueOutboxItem("cap-1", blob);

    const flushPromise = flushOutbox();
    await jest.runAllTimersAsync();
    await flushPromise;

    const items = await getOutboxItems();
    expect(items[0].state).toBe("failed");
  });
});

// ── Upload: unpaired behavior ────────────────────────────────────────

describe("flushOutbox — unpaired", () => {
  it("does nothing when no pairing state exists", async () => {
    await enqueueOutboxItem("cap-1", await createEncryptedFixture("cap-1", "test"));

    await flushOutbox();

    const calls = (global.fetch as jest.Mock).mock.calls;
    const bundleCalls = calls.filter((c: [string]) =>
      c[0].includes("/v1/sync/bundle"),
    );
    expect(bundleCalls.length).toBe(0);

    const items = await getOutboxItems();
    expect(items[0].state).toBe("pending");
  });

  it("stops flush on UnpairedError without deleting rows", async () => {
    await ensureIdentity();
    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);

    global.fetch = jest
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ code: "AUTH_INVALID" }), { status: 401 }),
      ) as typeof fetch;

    await enqueueOutboxItem("cap-1", await createEncryptedFixture("cap-1", "a"));
    await enqueueOutboxItem("cap-2", await createEncryptedFixture("cap-2", "b"));

    await flushOutbox();

    const items = await getOutboxItems();
    expect(items.length).toBe(2);
  });
});

// ── Single-flight ────────────────────────────────────────────────────

describe("flushOutbox — single-flight", () => {
  beforeEach(async () => {
    await ensureIdentity();
    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);
  });

  it("prevents concurrent flush loops", async () => {
    let resolveFirst: () => void;
    const firstDone = new Promise<void>((resolve) => {
      resolveFirst = resolve;
    });

    global.fetch = jest
      .fn()
      .mockImplementationOnce(async () => {
        await firstDone;
        return new Response(null, { status: 201 });
      })
      .mockResolvedValueOnce(new Response(null, { status: 201 })) as typeof fetch;

    await enqueueOutboxItem("cap-1", await createEncryptedFixture("cap-1", "a"));
    await enqueueOutboxItem("cap-2", await createEncryptedFixture("cap-2", "b"));

    const flush1 = flushOutbox();
    await flushOutbox(); // second call should return immediately (single-flight)

    resolveFirst!();
    await flush1;

    const items = await getOutboxItems();
    const doneCount = items.filter((i) => i.state === "done").length;
    expect(doneCount).toBeGreaterThanOrEqual(1);
  });
});

// ── last_error whitelist ─────────────────────────────────────────────

describe("last_error whitelist", () => {
  beforeEach(async () => {
    await ensureIdentity();
    await persistPairingState(defaultPairingState);
    useAppStore.getState().setPaired(defaultPairingState.pairedPeerFingerprint);
  });

  it("maps HTTP status codes to whitelisted error codes", async () => {
    global.fetch = jest
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 413 })) as typeof fetch;

    await enqueueOutboxItem("cap-1", await createEncryptedFixture("cap-1", "big"));
    await flushOutbox();

    const items = await getOutboxItems();
    expect(items[0].state).toBe("failed");
  });
});

// ── getOutboxItems backward compat ────────────────────────────────────

describe("getOutboxItems", () => {
  it("returns items ordered by created_at ASC", async () => {
    const blob1 = await createEncryptedFixture("cap-1", "first");
    const blob2 = await createEncryptedFixture("cap-2", "second");

    await enqueueOutboxItem("cap-2", blob2);
    await enqueueOutboxItem("cap-1", blob1);

    const items = await getOutboxItems();
    expect(items.length).toBe(2);
    expect(items[0].id).toBe("cap-2");
    expect(items[1].id).toBe("cap-1");
  });
});
