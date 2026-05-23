# syncmind-storage

SQLite persistence for indexed files, chunks, vectors, and local desktop state.

## Pinned Chunks

Pinned search results are stored in `pinned_chunks`:

```sql
CREATE TABLE IF NOT EXISTS pinned_chunks (
    chunk_id INTEGER PRIMARY KEY,
    pinned_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    FOREIGN KEY (chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_pinned_chunks_pinned_at
    ON pinned_chunks(pinned_at DESC);
```

`chunk_id` references `chunks(id)`, so deleting a chunk or deleting its source
file cascades and removes the pin automatically. Pins are local to the SQLite
database and do not introduce any sync behavior.

`VectorStore::list_pinned_chunks` joins `pinned_chunks`, `chunks`, and `files`
to return the same `SearchResult` payload shape used by search results. Pinned
rows are ordered by `pinned_at DESC` and use a synthetic `score` of `1.0`
because they bypass vector ranking.
