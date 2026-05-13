use crate::error::StorageError;
use crate::models::{Chunk, FileMeta, SearchResult};
use rusqlite::{params, Connection, OptionalExtension};
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
            );",
        )?;

        let vec_table_sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
                chunk_id INTEGER PRIMARY KEY,
                embedding FLOAT32[{}]
            );",
            self.embedding_dim
        );
        conn.execute(&vec_table_sql, [])?;

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
            for chunk_id in chunk_ids {
                tx.execute("DELETE FROM vec_chunks WHERE chunk_id = ?", [chunk_id])?;
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

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub fn get_stats(&self) -> Result<(usize, usize), StorageError> {
        let conn = self.conn.lock().unwrap();
        let file_count: usize = conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        let chunk_count: usize = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
        Ok((file_count, chunk_count))
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
}
