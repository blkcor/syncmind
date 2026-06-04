import * as SQLite from "expo-sqlite";
import { authenticatedFetch, UnpairedError } from "../spine/client";
import { getRestoredPairingState, type PersistedPairingState } from "../spine/session";

export class QueueFullError extends Error {
  constructor() {
    super("Capture queue is full - connect to upload or retry failed captures");
    this.name = "QueueFullError";
  }
}

export type OutboxState = "pending" | "sending" | "failed" | "done";

export interface OutboxRow {
  id: string;
  created_at: string;
  state: OutboxState;
  attempts: number;
  last_error: string | null;
  encrypted_blob: Uint8Array;
}

export interface OutboxStatusRow {
  id: string;
  created_at: string;
  state: OutboxState;
  attempts: number;
  last_error: string | null;
  preview_text: string | null;
}

export interface FlushOutboxResult {
  attemptedUploads: number;
}

interface OutboxDbRow {
  id: string;
  created_at: string;
  state: OutboxState;
  attempts: number;
  last_error: string | null;
  encrypted_blob: number[] | Uint8Array;
}

const WHITELISTED_ERRORS = [
  "HTTP_400", "HTTP_401", "HTTP_403", "HTTP_404", "HTTP_409",
  "HTTP_413", "HTTP_415", "HTTP_422", "HTTP_429",
  "HTTP_500", "HTTP_502", "HTTP_503", "HTTP_504",
  "NETWORK_ERROR", "UNPAIRED", "QUEUE_FULL", "BUNDLE_TOO_LARGE", "UNKNOWN_ERROR",
] as const;

export type WhitelistedError = (typeof WHITELISTED_ERRORS)[number];

function toWhitelistedError(code: string | undefined | null): WhitelistedError {
  if (code && (WHITELISTED_ERRORS as readonly string[]).includes(code)) {
    return code as WhitelistedError;
  }
  return "UNKNOWN_ERROR";
}

function httpStatusToLastError(status: number): WhitelistedError {
  const code = `HTTP_${status}`;
  return toWhitelistedError(code);
}

const MAX_UNFINISHED = 1000;
const DONE_RETENTION_DAYS = 7;
const CAPTURE_TEXT_CONTENT_TYPE = "application/syncmind.capture-text+json";

let db: SQLite.SQLiteDatabase | null = null;
const outboxListeners = new Set<() => void>();

function notifyOutboxChanged(): void {
  for (const listener of outboxListeners) {
    listener();
  }
}

export function subscribeToOutboxChanges(listener: () => void): () => void {
  outboxListeners.add(listener);
  return () => {
    outboxListeners.delete(listener);
  };
}

async function getDb(): Promise<SQLite.SQLiteDatabase> {
  if (!db) {
    db = await SQLite.openDatabaseAsync("syncmind_outbox.db");
    await db.execAsync(`
      CREATE TABLE IF NOT EXISTS outbox (
        id TEXT PRIMARY KEY,
        created_at TEXT NOT NULL,
        state TEXT NOT NULL CHECK (state IN ('pending', 'sending', 'failed', 'done')),
        attempts INTEGER NOT NULL DEFAULT 0,
        last_error TEXT,
        encrypted_blob BLOB NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_outbox_state_created ON outbox(state, created_at);
    `);

    const cols = await db.getAllAsync<{ name: string }>(
      "PRAGMA table_info(outbox)",
    );
    if (!cols.some((c) => c.name === "preview_text")) {
      await db.execAsync("ALTER TABLE outbox ADD COLUMN preview_text TEXT");
    }
  }
  return db;
}

function rowFromDb(r: OutboxDbRow): OutboxRow {
  const blob = Array.isArray(r.encrypted_blob)
    ? new Uint8Array(r.encrypted_blob)
    : r.encrypted_blob;
  return { ...r, encrypted_blob: blob };
}

export async function initOutbox(): Promise<void> {
  await getDb();
  await resetSendingToPending();
  await cleanupDoneRows();
}

export async function __closeOutboxForTests(): Promise<void> {
  if (db) {
    await db.closeAsync();
    db = null;
  }
  outboxListeners.clear();
}

export async function enqueueOutboxItem(
  id: string,
  blob: Uint8Array,
  preview?: string,
): Promise<void> {
  const d = await getDb();

  const row = await d.getFirstAsync<{ cnt: number }>(
    "SELECT COUNT(*) as cnt FROM outbox WHERE state IN ('pending', 'sending', 'failed')",
  );
  if (row && row.cnt >= MAX_UNFINISHED) {
    throw new QueueFullError();
  }

  const previewText = preview ? preview.slice(0, 100) : null;
  const createdAt = new Date().toISOString();
  await d.runAsync(
    "INSERT INTO outbox (id, created_at, state, attempts, last_error, encrypted_blob, preview_text) VALUES (?, ?, 'pending', 0, NULL, ?, ?)",
    id,
    createdAt,
    blob,
    previewText,
  );
  notifyOutboxChanged();
}

export async function clearOutbox(): Promise<void> {
  const d = await getDb();
  await d.runAsync("DELETE FROM outbox");
  notifyOutboxChanged();
}

export async function resetSendingToPending(): Promise<void> {
  const d = await getDb();
  await d.runAsync("UPDATE outbox SET state = 'pending' WHERE state = 'sending'");
  notifyOutboxChanged();
}

export async function cleanupDoneRows(): Promise<void> {
  const d = await getDb();
  const cutoff = new Date(Date.now() - DONE_RETENTION_DAYS * 86_400_000).toISOString();
  await d.runAsync("DELETE FROM outbox WHERE state = 'done' AND created_at < ?", cutoff);
  notifyOutboxChanged();
}

export async function getPendingCount(): Promise<number> {
  const d = await getDb();
  const row = await d.getFirstAsync<{ cnt: number }>(
    "SELECT COUNT(*) as cnt FROM outbox WHERE state IN ('pending', 'sending', 'failed')",
  );
  return row?.cnt ?? 0;
}

export async function getOutboxItems(): Promise<{ id: string; state: string }[]> {
  const d = await getDb();
  const rows = await d.getAllAsync<{ id: string; state: string }>(
    "SELECT id, state FROM outbox ORDER BY created_at ASC",
  );
  return rows.map((r) => ({ id: r.id, state: r.state }));
}

export async function getRecentOutboxStatuses(limit = 3): Promise<OutboxStatusRow[]> {
  const d = await getDb();
  const rows = await d.getAllAsync<OutboxStatusRow>(
    "SELECT id, created_at, state, attempts, last_error, preview_text FROM outbox ORDER BY created_at DESC LIMIT ?",
    limit,
  );
  return rows;
}

async function getNextPending(): Promise<OutboxRow | null> {
  const d = await getDb();
  const row = await d.getFirstAsync<OutboxDbRow>(
    "SELECT * FROM outbox WHERE state = 'pending' ORDER BY created_at ASC LIMIT 1",
  );
  return row ? rowFromDb(row) : null;
}

async function markSending(id: string): Promise<void> {
  const d = await getDb();
  await d.runAsync("UPDATE outbox SET state = 'sending' WHERE id = ?", id);
  notifyOutboxChanged();
}

async function incrementAttempts(id: string): Promise<void> {
  const d = await getDb();
  await d.runAsync(
    "UPDATE outbox SET attempts = attempts + 1 WHERE id = ?",
    id,
  );
}

async function markDone(id: string): Promise<void> {
  const d = await getDb();
  await d.runAsync("UPDATE outbox SET state = 'done', last_error = NULL WHERE id = ?", id);
  notifyOutboxChanged();
}

async function markFailed(id: string, error: WhitelistedError): Promise<void> {
  const d = await getDb();
  await d.runAsync(
    "UPDATE outbox SET state = 'failed', last_error = ? WHERE id = ?",
    error,
    id,
  );
  notifyOutboxChanged();
}

const RETRYABLE_STATUSES = new Set([429, 500, 502, 503, 504]);
const RETRY_DELAYS_MS = [1_000, 4_000, 16_000];
const MAX_ATTEMPTS = 3;

let flushing = false;

/**
 * Single-flight FIFO flush loop. Uploads pending rows to Spine in order.
 * Retries 429/5xx up to 3 attempts with backoff. Marks non-retryable or
 * exhausted rows as failed. Stops on missing pairing or UnpairedError.
 */
export async function flushOutbox(): Promise<FlushOutboxResult> {
  if (flushing) return { attemptedUploads: 0 };

  const state = getRestoredPairingState();
  if (!state) return { attemptedUploads: 0 };

  flushing = true;
  let attemptedUploads = 0;
  try {
    await resetSendingToPending();

    let row = await getNextPending();
    while (row) {
      const currentState = getRestoredPairingState();
      if (!currentState) break;

      const result = await tryUploadRow(row, currentState);
      attemptedUploads += result.attemptedUploads;
      if (!result.shouldContinue) break;

      row = await getNextPending();
    }
  } finally {
    flushing = false;
    await cleanupDoneRows();
  }

  return { attemptedUploads };
}

async function tryUploadRow(
  row: OutboxRow,
  state: PersistedPairingState,
): Promise<{ shouldContinue: boolean; attemptedUploads: number }> {
  await markSending(row.id);

  let lastAttempt = 1;
  let attemptedUploads = 0;

  while (lastAttempt <= MAX_ATTEMPTS) {
    try {
      await incrementAttempts(row.id);
      attemptedUploads++;
      const response = await authenticatedFetch(
        `${state.spineUrl}/v1/sync/bundle`,
        {
          method: "POST",
          headers: {
            "Content-Type": "application/octet-stream",
            "X-Syncmind-Content-Type": CAPTURE_TEXT_CONTENT_TYPE,
            "Idempotency-Key": row.id,
          },
          body: row.encrypted_blob as unknown as BodyInit,
        },
      );

      if (response.status === 201) {
        await markDone(row.id);
        return { shouldContinue: true, attemptedUploads };
      }

      if (RETRYABLE_STATUSES.has(response.status)) {
        if (lastAttempt >= MAX_ATTEMPTS) {
          await markFailed(row.id, httpStatusToLastError(response.status));
          return { shouldContinue: true, attemptedUploads };
        }
        await delay(RETRY_DELAYS_MS[lastAttempt - 1] ?? 16_000);
        lastAttempt++;
        continue;
      }

      // Non-retryable 4xx
      await markFailed(row.id, httpStatusToLastError(response.status));
      return { shouldContinue: true, attemptedUploads };
    } catch (err) {
      if (err instanceof UnpairedError) {
        return { shouldContinue: false, attemptedUploads }; // stop flushing, leave rows intact
      }

      if (lastAttempt >= MAX_ATTEMPTS) {
        await markFailed(row.id, "NETWORK_ERROR");
        return { shouldContinue: true, attemptedUploads };
      }

      await delay(RETRY_DELAYS_MS[lastAttempt - 1] ?? 16_000);
      lastAttempt++;
    }
  }

  await markFailed(row.id, "NETWORK_ERROR");
  return { shouldContinue: true, attemptedUploads };
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function __resetFlushLockForTests(): void {
  flushing = false;
}

export async function getOutboxRowsForTests(): Promise<OutboxRow[]> {
  const d = await getDb();
  const rows = await d.getAllAsync<OutboxDbRow>(
    "SELECT * FROM outbox ORDER BY created_at ASC",
  );
  return rows.map(rowFromDb);
}
