use crate::error::StorageError;
use crate::models::{Chunk, FileMeta, SearchResult};
use rusqlite::{params, Connection, OptionalExtension};
use sqlite_vec::sqlite3_vec_init;
use std::path::{Path, PathBuf};
use zerocopy::IntoBytes;

pub struct VectorStore {
    conn: Connection,
    embedding_dim: usize,
}

impl VectorStore {
    pub fn new(db_path: &Path, embedding_dim: usize) -> Result<Self, StorageError> {
        #[allow(clippy::missing_transmute_annotations)]
        unsafe {
            let result = rusqlite::ffi::sqlite3_auto_extension(Some(
                std::mem::transmute(sqlite3_vec_init as *const ()),
            ));
            if result != rusqlite::ffi::SQLITE_OK {
                return Err(StorageError::ExtensionRegistrationFailed);
            }
        }
        let conn = Connection::open(db_path)?;
        let store = Self { conn, embedding_dim };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), StorageError> {
        self.conn.execute_batch(
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
        self.conn.execute(&vec_table_sql, [])?;

        Ok(())
    }

    pub fn upsert_file(
        &mut self,
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

        let tx = self.conn.transaction()?;

        let file_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM files WHERE absolute_path = ?",
                [meta.absolute_path.to_string_lossy().as_ref()],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = file_id {
            tx.execute("DELETE FROM vec_chunks WHERE chunk_id IN (SELECT id FROM chunks WHERE file_id = ?)", [id])?;
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
                    chunk.chunk_index,
                    chunk.start_line,
                    chunk.end_line,
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

        let mut stmt = self.conn.prepare(
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
        let file_count: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        let chunk_count: usize = self
            .conn
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
        let mut store = VectorStore::new(&db_path, 4).unwrap();

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
        let mut store = VectorStore::new(&db_path, 4).unwrap();

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
        let mut store = VectorStore::new(&db_path, 4).unwrap();

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
