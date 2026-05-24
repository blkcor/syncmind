use crate::error::StorageError;
use crate::models::{Chunk, FileMeta, SearchResult};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use zerocopy::IntoBytes;

/// Raw SQLite extension entry point signature expected by `sqlite3_auto_extension`.
type SqliteInitFn = unsafe extern "C" fn(
    db: *mut rusqlite::ffi::sqlite3,
    pz_err_msg: *mut *const std::os::raw::c_char,
    p_api: *const rusqlite::ffi::sqlite3_api_routines,
) -> std::os::raw::c_int;

/// Register the sqlite-vec extension so it is automatically loaded into every
/// new database connection. This is `unsafe` because SQLite invokes the
/// function pointer from C code.
fn register_vec_extension() -> Result<(), StorageError> {
    // Reference the crate's symbol to ensure the #[link(name = "sqlite_vec0")]
    // attribute is activated and the extension is linked into the binary.
    let _ensure_linked = sqlite_vec::sqlite3_vec_init as *const ();

    // Declare the init function with the exact C ABI SQLite expects.
    // sqlite-vec is compiled with SQLITE_CORE and exports this standard
    // extension entry point. The signatures match, so the pointer cast is safe.
    extern "C" {
        fn sqlite3_vec_init(
            db: *mut rusqlite::ffi::sqlite3,
            pz_err_msg: *mut *const std::os::raw::c_char,
            p_api: *const rusqlite::ffi::sqlite3_api_routines,
        ) -> std::os::raw::c_int;
    }

    let result = unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(sqlite3_vec_init as SqliteInitFn))
    };

    if result != rusqlite::ffi::SQLITE_OK {
        return Err(StorageError::ExtensionRegistrationFailed);
    }
    Ok(())
}

pub struct VectorStore {
    conn: Mutex<Connection>,
    embedding_dim: usize,
}

impl VectorStore {
    pub fn new(db_path: &Path, embedding_dim: usize) -> Result<Self, StorageError> {
        register_vec_extension()?;
        let conn = Connection::open(db_path)?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        let store = Self {
            conn: Mutex::new(conn),
            embedding_dim,
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                absolute_path TEXT UNIQUE NOT NULL,
                file_type TEXT NOT NULL,
                last_modified INTEGER NOT NULL,
                last_indexed INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                chunk_index INTEGER NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS pinned_chunks (
                chunk_id INTEGER PRIMARY KEY,
                pinned_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
                FOREIGN KEY (chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_pinned_chunks_pinned_at
                ON pinned_chunks(pinned_at DESC);",
        )?;

        let vec_table_sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
                chunk_id INTEGER PRIMARY KEY,
                embedding FLOAT32[{}]
            );",
            self.embedding_dim
        );
        conn.execute(&vec_table_sql, [])?;

        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS fts_chunks USING fts5(content);",
            [],
        )?;

        Ok(())
    }

    pub fn upsert_file(
        &self,
        meta: &FileMeta,
        chunks: &[Chunk],
        embeddings: &[Vec<f32>],
    ) -> Result<(), StorageError> {
        if chunks.len() != embeddings.len() {
            return Err(StorageError::CountMismatch {
                chunks: chunks.len(),
                embeddings: embeddings.len(),
            });
        }

        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;

        let file_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM files WHERE absolute_path = ?",
                [meta.absolute_path.to_string_lossy().as_ref()],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = file_id {
            let chunk_ids: Vec<i64> = tx
                .prepare("SELECT id FROM chunks WHERE file_id = ?")?
                .query_map([id], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            for chunk_id in &chunk_ids {
                tx.execute("DELETE FROM vec_chunks WHERE chunk_id = ?", [chunk_id])?;
                tx.execute("DELETE FROM fts_chunks WHERE rowid = ?", [chunk_id])?;
            }
            tx.execute("DELETE FROM chunks WHERE file_id = ?", [id])?;
            tx.execute("DELETE FROM files WHERE id = ?", [id])?;
        }

        tx.execute(
            "INSERT INTO files (absolute_path, file_type, last_modified, last_indexed)
             VALUES (?, ?, ?, ?)",
            params![
                meta.absolute_path.to_string_lossy().as_ref(),
                &meta.file_type,
                meta.last_modified,
                meta.last_indexed,
            ],
        )?;
        let file_id = tx.last_insert_rowid();

        for (chunk, embedding) in chunks.iter().zip(embeddings.iter()) {
            if embedding.len() != self.embedding_dim {
                return Err(StorageError::InvalidDimension {
                    expected: self.embedding_dim,
                    actual: embedding.len(),
                });
            }

            tx.execute(
                "INSERT INTO chunks (file_id, chunk_index, start_line, end_line, content)
                 VALUES (?, ?, ?, ?, ?)",
                params![
                    file_id,
                    chunk.chunk_index as i64,
                    chunk.start_line as i64,
                    chunk.end_line as i64,
                    &chunk.content,
                ],
            )?;
            let chunk_id = tx.last_insert_rowid();

            tx.execute(
                "INSERT INTO vec_chunks (chunk_id, embedding) VALUES (?, ?)",
                params![chunk_id, embedding.as_bytes()],
            )?;

            tx.execute(
                "INSERT INTO fts_chunks (rowid, content) VALUES (?, ?)",
                params![chunk_id, &chunk.content],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn search(
        &self,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<SearchResult>, StorageError> {
        if query_embedding.len() != self.embedding_dim {
            return Err(StorageError::InvalidDimension {
                expected: self.embedding_dim,
                actual: query_embedding.len(),
            });
        }

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT
                c.id,
                c.start_line,
                c.end_line,
                c.content,
                f.absolute_path,
                vc.distance
             FROM vec_chunks vc
             JOIN chunks c ON vc.chunk_id = c.id
             JOIN files f ON c.file_id = f.id
             WHERE vc.embedding MATCH ? AND k = ?
             ORDER BY vc.distance
             LIMIT ?",
        )?;

        let rows = stmt.query_map(
            params![query_embedding.as_bytes(), top_k as i64, top_k as i64],
            |row| {
                let distance: f64 = row.get(5)?;
                Ok(SearchResult {
                    chunk_id: row.get(0)?,
                    start_line: row.get(1)?,
                    end_line: row.get(2)?,
                    content: row.get(3)?,
                    file_path: PathBuf::from(row.get::<_, String>(4)?),
                    score: Self::l2_to_similarity(distance),
                })
            },
        )?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    /// Convert an L2 distance (from sqlite-vec on normalized vectors) to an
    /// approximate cosine similarity score in [0, 1].
    fn l2_to_similarity(distance: f64) -> f64 {
        // For unit-length vectors: L2^2 = 2 - 2*dot_product
        // dot_product = 1 - L2^2/2
        let sim = 1.0 - (distance * distance) / 2.0;
        sim.clamp(0.0, 1.0)
    }

    /// Like `search`, but applies a path predicate to the candidate set before
    /// truncating to `top_k`. The vector search is over-fetched by a factor of
    /// `overfetch` to leave room for filtered-out rows; if every result is
    /// filtered out, the returned vector is empty.
    ///
    /// The path predicate receives the absolute path of each candidate chunk's
    /// source file. Returning `true` keeps the candidate.
    pub fn search_with_path_filter<F>(
        &self,
        query_embedding: &[f32],
        top_k: usize,
        overfetch: usize,
        keep: F,
    ) -> Result<Vec<SearchResult>, StorageError>
    where
        F: Fn(&Path) -> bool,
    {
        let target = top_k.saturating_mul(overfetch.max(1));
        let raw = self.search(query_embedding, target)?;
        Ok(raw
            .into_iter()
            .filter(|r| keep(&r.file_path))
            .take(top_k)
            .collect())
    }

    pub fn search_with_threshold(
        &self,
        query_embedding: &[f32],
        top_k: usize,
        threshold: Option<f64>,
    ) -> Result<Vec<SearchResult>, StorageError> {
        let mut results = self.search(query_embedding, top_k)?;
        if let Some(th) = threshold {
            results.retain(|r| r.score >= th);
        }
        Ok(results)
    }

    pub fn search_hybrid(
        &self,
        query_embedding: &[f32],
        query_text: &str,
        top_k: usize,
        threshold: Option<f64>,
    ) -> Result<Vec<SearchResult>, StorageError> {
        if query_embedding.len() != self.embedding_dim {
            return Err(StorageError::InvalidDimension {
                expected: self.embedding_dim,
                actual: query_embedding.len(),
            });
        }

        let conn = self.conn.lock().unwrap();
        let candidate_limit = (top_k * 2).max(10);
        let k_rrf = 60.0;

        // --- Vector candidates ---
        let mut vec_stmt = conn.prepare(
            "SELECT
                c.id,
                c.start_line,
                c.end_line,
                c.content,
                f.absolute_path,
                vc.distance
             FROM vec_chunks vc
             JOIN chunks c ON vc.chunk_id = c.id
             JOIN files f ON c.file_id = f.id
             WHERE vc.embedding MATCH ? AND k = ?
             ORDER BY vc.distance
             LIMIT ?",
        )?;

        let vec_rows = vec_stmt.query_map(
            params![
                query_embedding.as_bytes(),
                candidate_limit as i64,
                candidate_limit as i64
            ],
            |row| {
                Ok(SearchResult {
                    chunk_id: row.get(0)?,
                    start_line: row.get(1)?,
                    end_line: row.get(2)?,
                    content: row.get(3)?,
                    file_path: PathBuf::from(row.get::<_, String>(4)?),
                    score: row.get(5)?,
                })
            },
        )?;

        // --- FTS5 candidates ---
        let mut fts_stmt = conn.prepare(
            "SELECT
                c.id,
                c.start_line,
                c.end_line,
                c.content,
                f.absolute_path,
                rank
             FROM fts_chunks
             JOIN chunks c ON fts_chunks.rowid = c.id
             JOIN files f ON c.file_id = f.id
             WHERE fts_chunks MATCH ?
             ORDER BY rank
             LIMIT ?",
        )?;

        let fts_rows = fts_stmt.query_map(
            params![query_text, candidate_limit as i64],
            |row| {
                Ok(SearchResult {
                    chunk_id: row.get(0)?,
                    start_line: row.get(1)?,
                    end_line: row.get(2)?,
                    content: row.get(3)?,
                    file_path: PathBuf::from(row.get::<_, String>(4)?),
                    score: row.get(5)?,
                })
            },
        )?;

        // --- RRF fusion ---
        let mut fused: HashMap<i64, (SearchResult, f64)> = HashMap::new();

        for (rank, row) in vec_rows.enumerate() {
            let result = row?;
            let score = 1.0 / (k_rrf + rank as f64 + 1.0);
            fused.entry(result.chunk_id)
                .and_modify(|(_, s)| *s += score)
                .or_insert((result, score));
        }

        for (rank, row) in fts_rows.enumerate() {
            let result = row?;
            let score = 1.0 / (k_rrf + rank as f64 + 1.0);
            fused.entry(result.chunk_id)
                .and_modify(|(_, s)| *s += score)
                .or_insert((result, score));
        }

        let mut results: Vec<(SearchResult, f64)> = fused.into_values().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Apply threshold and top_k
        let mut final_results: Vec<SearchResult> = results
            .into_iter()
            .filter(|(r, _)| {
                if let Some(th) = threshold {
                    Self::l2_to_similarity(r.score) >= th
                } else {
                    true
                }
            })
            .map(|(mut r, rrf_score)| {
                // Overwrite score with the fused RRF score for downstream consumers
                r.score = rrf_score;
                r
            })
            .take(top_k)
            .collect();

        // Normalize scores to [0, 1] for downstream consistency.
        // RRF scores are small fractions; we scale by the max so the best result is ~1.0.
        if let Some(max_score) = final_results.first().map(|r| r.score) {
            if max_score > 0.0 {
                for r in &mut final_results {
                    r.score /= max_score;
                }
            }
        }

        Ok(final_results)
    }

    pub fn get_stats(&self) -> Result<(usize, usize), StorageError> {
        let conn = self.conn.lock().unwrap();
        let file_count: usize = conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        let chunk_count: usize = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
        Ok((file_count, chunk_count))
    }

    /// Delete a file and all its associated chunks and vectors from the store.
    ///
    /// Returns `Ok(true)` if a row was deleted, `Ok(false)` if no file with
    /// the given absolute path existed. The deletion is transactional and
    /// idempotent.
    ///
    /// `vec_chunks` is a sqlite-vec virtual table and does not honor the
    /// foreign-key cascade on `chunks`, so each linked row must be deleted
    /// explicitly before the parent rows are removed.
    pub fn delete_file_by_path(&self, path: &Path) -> Result<bool, StorageError> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;

        let file_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM files WHERE absolute_path = ?",
                [path.to_string_lossy().as_ref()],
                |row| row.get(0),
            )
            .optional()?;

        let Some(file_id) = file_id else {
            return Ok(false);
        };

        let chunk_ids: Vec<i64> = tx
            .prepare("SELECT id FROM chunks WHERE file_id = ?")?
            .query_map([file_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        for chunk_id in chunk_ids {
            tx.execute("DELETE FROM vec_chunks WHERE chunk_id = ?", [chunk_id])?;
        }
        tx.execute("DELETE FROM chunks WHERE file_id = ?", [file_id])?;
        tx.execute("DELETE FROM files WHERE id = ?", [file_id])?;

        tx.commit()?;
        Ok(true)
    }

    /// Pin a chunk so it persists in the user's quick-access list.
    ///
    /// Idempotent: pinning an already-pinned chunk succeeds without modifying
    /// `pinned_at`. Returns `Ok(())` even if the chunk row does not exist; the
    /// foreign-key cascade will surface as a different error path when the
    /// chunk is missing, depending on whether `PRAGMA foreign_keys` is on.
    pub fn pin_chunk(&self, chunk_id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO pinned_chunks (chunk_id) VALUES (?)",
            [chunk_id],
        )?;
        Ok(())
    }

    /// Remove a pin. Idempotent: unpinning a non-pinned chunk succeeds.
    pub fn unpin_chunk(&self, chunk_id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM pinned_chunks WHERE chunk_id = ?",
            [chunk_id],
        )?;
        Ok(())
    }

    /// Return `true` if the given chunk is currently pinned.
    pub fn is_chunk_pinned(&self, chunk_id: i64) -> Result<bool, StorageError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pinned_chunks WHERE chunk_id = ?",
            [chunk_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Bulk lookup: given a slice of chunk ids, return the subset that is
    /// currently pinned. Used by the result-row renderer so the palette does
    /// not issue N round-trips per page render.
    pub fn pinned_set(&self, chunk_ids: &[i64]) -> Result<HashSet<i64>, StorageError> {
        if chunk_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let conn = self.conn.lock().unwrap();
        let placeholders = std::iter::repeat_n("?", chunk_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT chunk_id FROM pinned_chunks WHERE chunk_id IN ({})",
            placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        let params_iter: Vec<&dyn rusqlite::ToSql> =
            chunk_ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params_iter.as_slice(), |row| row.get::<_, i64>(0))?;
        rows.collect::<Result<HashSet<_>, _>>()
            .map_err(StorageError::from)
    }

    /// List every currently-pinned chunk as a `SearchResult`, ordered by
    /// `pinned_at` descending (most recently pinned first). Score is set to a
    /// synthetic `1.0` since pinned items bypass vector ranking.
    pub fn list_pinned_chunks(&self) -> Result<Vec<SearchResult>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT
                c.id,
                c.start_line,
                c.end_line,
                c.content,
                f.absolute_path
             FROM pinned_chunks p
             JOIN chunks c ON p.chunk_id = c.id
             JOIN files f ON c.file_id = f.id
             ORDER BY p.pinned_at DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(SearchResult {
                chunk_id: row.get(0)?,
                start_line: row.get(1)?,
                end_line: row.get(2)?,
                content: row.get(3)?,
                file_path: PathBuf::from(row.get::<_, String>(4)?),
                score: 1.0,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    /// Return the distinct indexed file types currently present in the store.
    /// Results are normalized to lowercase, exclude empty/unknown values, and
    /// are ordered by frequency descending then alphabetically ascending.
    pub fn list_indexed_file_types(&self) -> Result<Vec<String>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT LOWER(file_type) AS normalized_type, COUNT(*) AS file_count
             FROM files
             WHERE TRIM(file_type) <> ''
               AND LOWER(file_type) <> 'unknown'
             GROUP BY normalized_type
             ORDER BY file_count DESC, normalized_type ASC",
        )?;

        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Chunk, FileMeta};
    use std::path::PathBuf;

    fn mock_embedding(dim: usize, value: f32) -> Vec<f32> {
        vec![value; dim]
    }

    #[test]
    fn store_init_and_upsert() {
        let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let store = VectorStore::new(&db_path, 4).unwrap();

        let meta = FileMeta {
            absolute_path: PathBuf::from("/tmp/test.md"),
            file_type: "markdown".to_string(),
            last_modified: 1234567890,
            last_indexed: 1234567890,
        };
        let chunks = vec![
            Chunk {
                chunk_index: 0,
                start_line: 1,
                end_line: 5,
                content: "Hello world".to_string(),
            },
        ];
        let embeddings = vec![mock_embedding(4, 0.1)];

        store.upsert_file(&meta, &chunks, &embeddings).unwrap();

        let (files, chunks_count) = store.get_stats().unwrap();
        assert_eq!(files, 1);
        assert_eq!(chunks_count, 1);
    }

    #[test]
    fn store_search_returns_results() {
        let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let store = VectorStore::new(&db_path, 4).unwrap();

        let meta = FileMeta {
            absolute_path: PathBuf::from("/tmp/test.md"),
            file_type: "markdown".to_string(),
            last_modified: 1234567890,
            last_indexed: 1234567890,
        };
        let chunks = vec![
            Chunk {
                chunk_index: 0,
                start_line: 1,
                end_line: 5,
                content: "Hello world".to_string(),
            },
            Chunk {
                chunk_index: 1,
                start_line: 6,
                end_line: 10,
                content: "Goodbye world".to_string(),
            },
        ];
        let embeddings = vec![
            vec![0.1, 0.2, 0.3, 0.4],
            vec![0.9, 0.8, 0.7, 0.6],
        ];

        store.upsert_file(&meta, &chunks, &embeddings).unwrap();

        let query = vec![0.11, 0.19, 0.31, 0.39];
        let results = store.search(&query, 2).unwrap();

        assert!(!results.is_empty());
        assert_eq!(results[0].content, "Hello world");
        assert!(
            (0.0..=1.0).contains(&results[0].score),
            "search score should be normalized similarity, got {}",
            results[0].score
        );
        assert!(
            results[0].score >= results[1].score,
            "closer result should have greater or equal similarity score"
        );
    }

    fn count_vec_chunks(store: &VectorStore) -> usize {
        let conn = store.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn store_delete_file_by_path_clears_all_artifacts() {
        let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let store = VectorStore::new(&db_path, 4).unwrap();

        let meta = FileMeta {
            absolute_path: PathBuf::from("/tmp/delete_me.md"),
            file_type: "markdown".to_string(),
            last_modified: 1,
            last_indexed: 1,
        };
        let chunks = vec![
            Chunk {
                chunk_index: 0,
                start_line: 1,
                end_line: 2,
                content: "First".to_string(),
            },
            Chunk {
                chunk_index: 1,
                start_line: 3,
                end_line: 4,
                content: "Second".to_string(),
            },
        ];
        let embeddings = vec![mock_embedding(4, 0.1), mock_embedding(4, 0.2)];
        store.upsert_file(&meta, &chunks, &embeddings).unwrap();

        let (files_before, chunks_before) = store.get_stats().unwrap();
        assert_eq!(files_before, 1);
        assert_eq!(chunks_before, 2);
        assert_eq!(count_vec_chunks(&store), 2);

        let removed = store
            .delete_file_by_path(&PathBuf::from("/tmp/delete_me.md"))
            .unwrap();
        assert!(removed);

        let (files_after, chunks_after) = store.get_stats().unwrap();
        assert_eq!(files_after, 0);
        assert_eq!(chunks_after, 0);
        assert_eq!(
            count_vec_chunks(&store),
            0,
            "vec_chunks must be cleared (sqlite-vec does not cascade)"
        );
    }

    #[test]
    fn store_delete_file_by_path_idempotent_for_unknown() {
        let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let store = VectorStore::new(&db_path, 4).unwrap();

        let removed = store
            .delete_file_by_path(&PathBuf::from("/tmp/never_indexed.md"))
            .unwrap();
        assert!(!removed);
    }

    #[test]
    fn store_upsert_replaces_existing_file() {
        let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let store = VectorStore::new(&db_path, 4).unwrap();

        let meta = FileMeta {
            absolute_path: PathBuf::from("/tmp/test.md"),
            file_type: "markdown".to_string(),
            last_modified: 1,
            last_indexed: 1,
        };
        let chunks = vec![Chunk {
            chunk_index: 0,
            start_line: 1,
            end_line: 2,
            content: "First".to_string(),
        }];
        let embeddings = vec![mock_embedding(4, 0.1)];

        store.upsert_file(&meta, &chunks, &embeddings).unwrap();
        store.upsert_file(&meta, &chunks, &embeddings).unwrap();

        let (files, chunks_count) = store.get_stats().unwrap();
        assert_eq!(files, 1);
        assert_eq!(chunks_count, 1);
    }

    #[test]
    fn store_search_with_threshold_filters_low_similarity() {
        let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let store = VectorStore::new(&db_path, 4).unwrap();

        let meta = FileMeta {
            absolute_path: PathBuf::from("/tmp/test.md"),
            file_type: "markdown".to_string(),
            last_modified: 1,
            last_indexed: 1,
        };
        let chunks = vec![
            Chunk {
                chunk_index: 0,
                start_line: 1,
                end_line: 2,
                content: "Hello world".to_string(),
            },
            Chunk {
                chunk_index: 1,
                start_line: 3,
                end_line: 4,
                content: "Goodbye world".to_string(),
            },
        ];
        // Very different embeddings: one close to query, one far
        let embeddings = vec![
            vec![0.1, 0.2, 0.3, 0.4],
            vec![0.9, 0.8, 0.7, 0.6],
        ];
        store.upsert_file(&meta, &chunks, &embeddings).unwrap();

        let query = vec![0.11, 0.19, 0.31, 0.39];
        // With a high threshold, only the very close result should remain
        let results = store.search_with_threshold(&query, 5, Some(0.95)).unwrap();
        assert_eq!(results.len(), 1, "threshold should filter out dissimilar results");
        assert_eq!(results[0].content, "Hello world");
    }

    #[test]
    fn store_search_hybrid_returns_fused_results() {
        let db_path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let store = VectorStore::new(&db_path, 4).unwrap();

        let meta = FileMeta {
            absolute_path: PathBuf::from("/tmp/test.md"),
            file_type: "markdown".to_string(),
            last_modified: 1,
            last_indexed: 1,
        };
        let chunks = vec![
            Chunk {
                chunk_index: 0,
                start_line: 1,
                end_line: 2,
                content: "Hello world".to_string(),
            },
            Chunk {
                chunk_index: 1,
                start_line: 3,
                end_line: 4,
                content: "Goodbye world".to_string(),
            },
        ];
        let embeddings = vec![
            vec![0.1, 0.2, 0.3, 0.4],
            vec![0.9, 0.8, 0.7, 0.6],
        ];
        store.upsert_file(&meta, &chunks, &embeddings).unwrap();

        let query_embedding = vec![0.11, 0.19, 0.31, 0.39];
        let results = store
            .search_hybrid(&query_embedding, "Hello", 5, None)
            .unwrap();

        assert!(
            !results.is_empty(),
            "hybrid search should return at least one result"
        );
        // The BM25 arm should boost "Hello world" to the top.
        assert_eq!(results[0].content, "Hello world");
    }

    // ---- Pin / unpin tests ----

    /// Seed two chunks and return (store, [chunk_id_a, chunk_id_b], _tmp).
    /// The `TempDir` must stay in scope for the duration of the test, otherwise
    /// the underlying SQLite file is unlinked and subsequent writes fail with
    /// "attempt to write a readonly database".
    fn seed_two_chunks() -> (VectorStore, Vec<i64>, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("syncmind.db");
        let store = VectorStore::new(&db_path, 4).unwrap();

        let meta = FileMeta {
            absolute_path: PathBuf::from("/tmp/pin_test.md"),
            file_type: "markdown".to_string(),
            last_modified: 1,
            last_indexed: 1,
        };
        let chunks = vec![
            Chunk {
                chunk_index: 0,
                start_line: 1,
                end_line: 2,
                content: "First".to_string(),
            },
            Chunk {
                chunk_index: 1,
                start_line: 3,
                end_line: 4,
                content: "Second".to_string(),
            },
        ];
        let embeddings = vec![mock_embedding(4, 0.1), mock_embedding(4, 0.2)];
        store.upsert_file(&meta, &chunks, &embeddings).unwrap();

        let ids: Vec<i64> = {
            let conn = store.conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT id FROM chunks ORDER BY id ASC").unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<Vec<i64>, _>>()
                .unwrap()
        };
        assert_eq!(ids.len(), 2, "fixture must produce exactly two chunk rows");
        (store, ids, tmp)
    }

    #[test]
    fn pin_chunk_is_idempotent() {
        let (store, ids, _tmp) = seed_two_chunks();

        store.pin_chunk(ids[0]).unwrap();
        // First pinned_at snapshot
        let first_ts: i64 = {
            let conn = store.conn.lock().unwrap();
            conn.query_row(
                "SELECT pinned_at FROM pinned_chunks WHERE chunk_id = ?",
                [ids[0]],
                |row| row.get(0),
            )
            .unwrap()
        };
        // Second pin: must succeed and leave pinned_at unchanged.
        store.pin_chunk(ids[0]).unwrap();
        let second_ts: i64 = {
            let conn = store.conn.lock().unwrap();
            conn.query_row(
                "SELECT pinned_at FROM pinned_chunks WHERE chunk_id = ?",
                [ids[0]],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(
            first_ts, second_ts,
            "repeated pin must not mutate pinned_at"
        );

        assert!(store.is_chunk_pinned(ids[0]).unwrap());
        assert!(!store.is_chunk_pinned(ids[1]).unwrap());
    }

    #[test]
    fn unpin_chunk_is_idempotent() {
        let (store, ids, _tmp) = seed_two_chunks();
        // Unpin a chunk that was never pinned -> Ok.
        store.unpin_chunk(ids[0]).unwrap();
        assert!(!store.is_chunk_pinned(ids[0]).unwrap());

        store.pin_chunk(ids[0]).unwrap();
        store.unpin_chunk(ids[0]).unwrap();
        store.unpin_chunk(ids[0]).unwrap(); // double-unpin Ok
        assert!(!store.is_chunk_pinned(ids[0]).unwrap());
    }

    #[test]
    fn pinned_set_returns_intersection() {
        let (store, ids, _tmp) = seed_two_chunks();
        store.pin_chunk(ids[1]).unwrap();

        let result = store
            .pinned_set(&[ids[0], ids[1], 999_999])
            .unwrap();
        assert!(!result.contains(&ids[0]));
        assert!(result.contains(&ids[1]));
        assert!(!result.contains(&999_999));
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn pinned_set_empty_input_returns_empty_set() {
        let (store, _ids, _tmp) = seed_two_chunks();
        let result = store.pinned_set(&[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn list_pinned_chunks_orders_by_pinned_at_desc() {
        let (store, ids, _tmp) = seed_two_chunks();
        store.pin_chunk(ids[0]).unwrap();
        // Sleep ~1.1s to cross a `strftime('%s','now')` second boundary, so
        // the two pinned_at values are strictly different and ordering is
        // unambiguous.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        store.pin_chunk(ids[1]).unwrap();

        let listed = store.list_pinned_chunks().unwrap();
        assert_eq!(listed.len(), 2);
        // Most recent (ids[1]) first.
        assert_eq!(listed[0].chunk_id, ids[1]);
        assert_eq!(listed[1].chunk_id, ids[0]);
        // Score is the synthetic 1.0 for pinned items.
        assert!((listed[0].score - 1.0).abs() < f64::EPSILON);
        assert_eq!(listed[0].file_path, PathBuf::from("/tmp/pin_test.md"));
        assert_eq!(listed[0].content, "Second");
    }

    #[test]
    fn deleting_underlying_file_cascades_pin() {
        let (store, ids, _tmp) = seed_two_chunks();
        store.pin_chunk(ids[0]).unwrap();
        store.pin_chunk(ids[1]).unwrap();
        assert_eq!(store.list_pinned_chunks().unwrap().len(), 2);

        let removed = store
            .delete_file_by_path(&PathBuf::from("/tmp/pin_test.md"))
            .unwrap();
        assert!(removed);

        let after = store.list_pinned_chunks().unwrap();
        assert!(
            after.is_empty(),
            "deleting parent file must cascade and clear all pins (got {} rows)",
            after.len()
        );
    }

    #[test]
    fn reindex_via_upsert_cascades_pin() {
        // Pin a chunk, then re-upsert the same file (replacing all chunks).
        // The original chunk row is deleted in the upsert transaction, which
        // must cascade and remove the pin.
        let (store, ids, _tmp) = seed_two_chunks();
        store.pin_chunk(ids[0]).unwrap();
        assert!(store.is_chunk_pinned(ids[0]).unwrap());

        let new_meta = FileMeta {
            absolute_path: PathBuf::from("/tmp/pin_test.md"),
            file_type: "markdown".to_string(),
            last_modified: 2,
            last_indexed: 2,
        };
        let new_chunks = vec![Chunk {
            chunk_index: 0,
            start_line: 1,
            end_line: 3,
            content: "Rewritten".to_string(),
        }];
        let new_embeddings = vec![mock_embedding(4, 0.3)];
        store
            .upsert_file(&new_meta, &new_chunks, &new_embeddings)
            .unwrap();

        assert!(
            store.list_pinned_chunks().unwrap().is_empty(),
            "re-indexing must cascade-clear pins on replaced chunks"
        );
    }

    #[test]
    fn schema_init_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("syncmind.db");
        let _store1 = VectorStore::new(&db_path, 4).unwrap();
        let _store2 = VectorStore::new(&db_path, 4).unwrap();
    }

    #[test]
    fn list_indexed_file_types_returns_distinct_normalized_extensions() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("syncmind.db");
        let store = VectorStore::new(&db_path, 4).unwrap();

        let fixtures = [
            ("/tmp/main.RS", "RS"),
            ("/tmp/lib.rs", "rs"),
            ("/tmp/readme.md", "md"),
            ("/tmp/notes.MD", "Md"),
            ("/tmp/guide.txt", "txt"),
            ("/tmp/unknown.bin", "unknown"),
            ("/tmp/blank", ""),
        ];

        for (idx, (path, file_type)) in fixtures.iter().enumerate() {
            let meta = FileMeta {
                absolute_path: PathBuf::from(path),
                file_type: (*file_type).to_string(),
                last_modified: idx as i64,
                last_indexed: idx as i64,
            };
            let chunks = vec![Chunk {
                chunk_index: 0,
                start_line: 1,
                end_line: 1,
                content: format!("fixture-{idx}"),
            }];
            let embeddings = vec![mock_embedding(4, idx as f32 + 0.1)];
            store.upsert_file(&meta, &chunks, &embeddings).unwrap();
        }

        let file_types = store.list_indexed_file_types().unwrap();
        assert_eq!(file_types, vec!["md", "rs", "txt"]);
    }

    #[test]
    fn list_indexed_file_types_sorts_by_frequency_then_alphabetically() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("syncmind.db");
        let store = VectorStore::new(&db_path, 4).unwrap();

        let fixtures = [
            ("/tmp/a.ts", "ts"),
            ("/tmp/b.ts", "TS"),
            ("/tmp/c.rs", "rs"),
            ("/tmp/d.md", "md"),
            ("/tmp/e.py", "py"),
        ];

        for (idx, (path, file_type)) in fixtures.iter().enumerate() {
            let meta = FileMeta {
                absolute_path: PathBuf::from(path),
                file_type: (*file_type).to_string(),
                last_modified: idx as i64,
                last_indexed: idx as i64,
            };
            let chunks = vec![Chunk {
                chunk_index: 0,
                start_line: 1,
                end_line: 1,
                content: format!("fixture-{idx}"),
            }];
            let embeddings = vec![mock_embedding(4, idx as f32 + 0.1)];
            store.upsert_file(&meta, &chunks, &embeddings).unwrap();
        }

        let file_types = store.list_indexed_file_types().unwrap();
        assert_eq!(file_types, vec!["ts", "md", "py", "rs"]);
    }
}
