use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use syncmind_mcp_server::protocol::*;
use syncmind_mcp_server::server::McpServer;
use syncmind_rag_engine::embedder::Embedder;
use syncmind_rag_engine::error::EmbedError;
use syncmind_storage::{Chunk, FileMeta, VectorStore};

struct MockEmbedder {
    dim: usize,
}

#[async_trait]
impl Embedder for MockEmbedder {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        Ok(texts.iter().map(|_| vec![0.0; self.dim]).collect())
    }

    fn embedding_dim(&self) -> usize {
        self.dim
    }
}

fn create_test_store() -> Arc<VectorStore> {
    Arc::new(VectorStore::new(Path::new(":memory:"), 384).unwrap())
}

fn create_server() -> McpServer {
    let store = create_test_store();
    let embedder = Arc::new(MockEmbedder { dim: 384 });
    McpServer::new(store, embedder)
}

#[tokio::test]
async fn test_initialize() {
    let server = create_server();

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "initialize".to_string(),
        params: None,
    };

    let resp = server.handle_request(req).await.unwrap();
    assert_eq!(resp.id, Some(json!(1)));

    let result = match resp.body {
        JsonRpcResponseBody::Result { result } => result,
        JsonRpcResponseBody::Error { error } => panic!("unexpected error: {:?}", error),
    };

    assert_eq!(result["protocol_version"], "2024-11-05");
    assert_eq!(result["server_info"]["name"], "syncmind");
    assert!(result["capabilities"]["tools"].is_object());
}

#[tokio::test]
async fn test_tools_list() {
    let server = create_server();

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "tools/list".to_string(),
        params: None,
    };

    let resp = server.handle_request(req).await.unwrap();
    let result = match resp.body {
        JsonRpcResponseBody::Result { result } => result,
        JsonRpcResponseBody::Error { error } => panic!("unexpected error: {:?}", error),
    };

    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "search_knowledge");
}

#[tokio::test]
async fn test_tools_call_search_knowledge_empty() {
    let server = create_server();

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "search_knowledge",
            "arguments": {
                "query": "rust async",
                "top_k": 3
            }
        })),
    };

    let resp = server.handle_request(req).await.unwrap();
    let result = match resp.body {
        JsonRpcResponseBody::Result { result } => result,
        JsonRpcResponseBody::Error { error } => panic!("unexpected error: {:?}", error),
    };

    let content = result["content"].as_array().unwrap();
    assert_eq!(content.len(), 1);
    let text = &content[0]["text"];
    let results: Vec<syncmind_storage::SearchResult> = serde_json::from_str(text.as_str().unwrap()).unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_tools_call_search_knowledge_with_results() {
    let store = create_test_store();
    let embedder = Arc::new(MockEmbedder { dim: 384 });

    // Seed the store with a file and chunk.
    let meta = FileMeta {
        absolute_path: Path::new("/tmp/test.rs").to_path_buf(),
        file_type: "rs".to_string(),
        last_modified: 0,
        last_indexed: 0,
    };
    let chunks = vec![Chunk {
        chunk_index: 0,
        content: "fn main() {}".to_string(),
        start_line: 1,
        end_line: 1,
    }];
    let embeddings = vec![vec![0.0; 384]];
    store.upsert_file(&meta, &chunks, &embeddings).unwrap();

    let server = McpServer::new(store, embedder);

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "search_knowledge",
            "arguments": {
                "query": "main function",
                "top_k": 5
            }
        })),
    };

    let resp = server.handle_request(req).await.unwrap();
    let result = match resp.body {
        JsonRpcResponseBody::Result { result } => result,
        JsonRpcResponseBody::Error { error } => panic!("unexpected error: {:?}", error),
    };

    let content = result["content"].as_array().unwrap();
    let text = content[0]["text"].as_str().unwrap();
    let results: Vec<syncmind_storage::SearchResult> = serde_json::from_str(text).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "fn main() {}");
}

#[tokio::test]
async fn test_tools_call_unknown_tool() {
    let server = create_server();

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "unknown_tool",
            "arguments": {}
        })),
    };

    let resp = server.handle_request(req).await.unwrap();
    match resp.body {
        JsonRpcResponseBody::Error { error } => {
            assert_eq!(error.code, -32601);
            assert!(error.message.contains("Tool not found"));
        }
        JsonRpcResponseBody::Result { .. } => panic!("expected error"),
    }
}

#[tokio::test]
async fn test_notification_has_no_response() {
    let server = create_server();

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: None,
        method: "initialize".to_string(),
        params: None,
    };

    let resp = server.handle_request(req).await;
    assert!(resp.is_none());
}
