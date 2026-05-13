use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::debug;

use crate::protocol::*;
use syncmind_rag_engine::embedder::Embedder;
use syncmind_storage::VectorStore;

/// A handler for an MCP tool.
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// Execute the tool with the given arguments.
    async fn call(&self, args: &Option<Value>) -> Result<CallToolResult, String>;
}

/// The SyncMind MCP server.
pub struct McpServer {
    store: Arc<VectorStore>,
    embedder: Arc<dyn Embedder>,
    tools: Vec<(Tool, Arc<dyn ToolHandler>)>,
}

impl McpServer {
    /// Create a new server with the given store and embedder.
    pub fn new(store: Arc<VectorStore>, embedder: Arc<dyn Embedder>) -> Self {
        let mut server = Self {
            store,
            embedder,
            tools: Vec::new(),
        };
        server.register_builtin_tools();
        server
    }

    /// Register the built-in `search_knowledge` tool.
    fn register_builtin_tools(&mut self) {
        let tool = Tool {
            name: "search_knowledge".to_string(),
            description: "Search the local knowledge base for relevant context chunks.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query text"
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Maximum number of results to return",
                        "default": 5
                    },
                    "filter_file_type": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional file type filters"
                    }
                },
                "required": ["query"]
            }),
        };

        let handler = SearchKnowledgeHandler {
            store: self.store.clone(),
            embedder: self.embedder.clone(),
        };

        self.tools.push((tool, Arc::new(handler)));
    }

    /// Register a custom tool.
    pub fn register_tool(&mut self, tool: Tool, handler: Arc<dyn ToolHandler>) {
        self.tools.push((tool, handler));
    }

    /// Handle a single JSON-RPC request and produce a response.
    pub async fn handle_request(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        debug!(method = %req.method, "handling MCP request");

        // Notifications (no id) don't get responses.
        let id = req.id.clone();

        let result = match req.method.as_str() {
            "initialize" => self.handle_initialize(req.params),
            "initialized" => Ok(Value::Null),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(req.params).await,
            _ => Err(JsonRpcError::method_not_found(format!(
                "Unknown method: {}",
                req.method
            ))),
        };

        // Notifications don't return responses.
        id.as_ref()?;

        let body = match result {
            Ok(value) => JsonRpcResponseBody::Result { result: value },
            Err(err) => JsonRpcResponseBody::Error { error: err },
        };

        Some(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            body,
        })
    }

    fn handle_initialize(&self, _params: Option<Value>) -> Result<Value, JsonRpcError> {
        serde_json::to_value(InitializeResult {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability { list_changed: false }),
            },
            server_info: Implementation {
                name: "syncmind".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        })
        .map_err(|e| JsonRpcError::internal_error(e.to_string()))
    }

    fn handle_tools_list(&self) -> Result<Value, JsonRpcError> {
        let tools: Vec<Tool> = self.tools.iter().map(|(t, _)| t.clone()).collect();
        serde_json::to_value(ListToolsResult { tools })
            .map_err(|e| JsonRpcError::internal_error(e.to_string()))
    }

    async fn handle_tools_call(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let params: CallToolParams = serde_json::from_value(params.unwrap_or(Value::Null))
            .map_err(|e| JsonRpcError::invalid_params(e.to_string()))?;

        let (_, handler) = self
            .tools
            .iter()
            .find(|(t, _)| t.name == params.name)
            .ok_or_else(|| {
                JsonRpcError::method_not_found(format!("Tool not found: {}", params.name))
            })?;

        match handler.call(&params.arguments).await {
            Ok(result) => Ok(serde_json::to_value(result)
                .map_err(|e| JsonRpcError::internal_error(e.to_string()))?),
            Err(e) => Ok(serde_json::to_value(CallToolResult::error(e))
                .map_err(|e| JsonRpcError::internal_error(e.to_string()))?),
        }
    }
}

struct SearchKnowledgeHandler {
    store: Arc<VectorStore>,
    embedder: Arc<dyn Embedder>,
}

#[async_trait]
impl ToolHandler for SearchKnowledgeHandler {
    async fn call(&self, args: &Option<Value>) -> Result<CallToolResult, String> {
        let args = args.as_ref().ok_or("Missing arguments")?;
        let query = args["query"]
            .as_str()
            .ok_or("Missing required parameter: query")?;
        let top_k = args["top_k"].as_u64().unwrap_or(5) as usize;
        if top_k == 0 || top_k > 100 {
            return Err("Invalid top_k: must be between 1 and 100".to_string());
        }

        let embeddings = self
            .embedder
            .embed(&[query])
            .await
            .map_err(|e| format!("Embedding failed: {}", e))?;
        let query_embedding = embeddings
            .into_iter()
            .next()
            .ok_or("Embedding returned empty result")?;

        let mut results = self
            .store
            .search(&query_embedding, top_k)
            .map_err(|e| format!("Search failed: {}", e))?;

        // Apply optional file type filter post-search.
        if let Some(filter) = args.get("filter_file_type").and_then(|f| f.as_array()) {
            let filters: Vec<String> = filter
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
                .collect();
            if !filters.is_empty() {
                results.retain(|r| {
                    r.file_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| filters.contains(&e.to_lowercase()))
                        .unwrap_or(false)
                });
            }
        }

        let text = serde_json::to_string_pretty(&results)
            .unwrap_or_else(|_| "[]".to_string());
        Ok(CallToolResult::text(text))
    }
}
