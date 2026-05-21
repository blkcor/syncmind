# SyncMind Phase 1 Quickstart

A 5-minute end-to-end setup: build the daemon, index a couple of files, and verify Claude Code can search them via MCP.

## Prerequisites

- macOS or Linux
- Rust toolchain (`rustup install stable`)
- (Optional) [Ollama](https://ollama.com) with `bge-m3` pulled — strongly recommended; the ONNX fallback works but is lower quality
- Claude Code installed and authenticated

## 1. Build the daemon

```bash
git clone https://github.com/blkcor-bt/syncmind
cd syncmind/core
cargo build --release --bin syncmind
sudo cp target/release/syncmind /usr/local/bin/
syncmind --help
```

## 2. Initialize config

```bash
syncmind status      # creates <config-dir>/syncmind/config.toml with defaults
```

If you are using Ollama (recommended), confirm the model:

```bash
ollama pull bge-m3   # default model
```

If you are NOT using Ollama, switch to the smaller 384-dim model in `<config-dir>/syncmind/config.toml`:

```toml
embedding_dim = 384
```

The first daemon launch will auto-download the ONNX model (~130 MB) to `<data-dir>/syncmind/models/`. You can also point at an in-country mirror via:

```toml
onnx_model_url = "https://your-mirror.example/bge-small/model.onnx"
onnx_tokenizer_url = "https://your-mirror.example/bge-small/tokenizer.json"
```

## 3. Register a few files

Always use absolute paths.

```bash
syncmind register "$HOME/notes/research.md"
syncmind register "$HOME/code/myproject/README.md"
syncmind register "$HOME/code/myproject/src/main.rs"
syncmind status      # confirm they're listed
```

## 4. Start the daemon (foreground for first run)

```bash
syncmind daemon --foreground
```

You should see:

```
INFO syncmind: Starting SyncMind daemon...
INFO syncmind_rag_engine::embedder: Using Ollama embedder at http://localhost:11434
INFO syncmind_indexing: indexed file path=/Users/.../notes/research.md
INFO syncmind_mcp_server::stdio: MCP stdio server ready
```

Logs are also written to `<data-dir>/syncmind/logs/syncmind.log.<YYYY-MM-DD>`.

## 5. CLI sanity-check the search

In another shell:

```bash
syncmind search "what did I write about embeddings"
```

You should see the top-5 chunks ranked by cosine similarity.

## 6. Wire SyncMind into Claude Code

Stop the foreground daemon (`Ctrl-C`) — Claude Code will start the daemon on demand via the MCP stdio handshake.

Add the MCP server using the example payload at `docs/examples/claude_code_mcp.json`:

```bash
claude mcp add-json syncmind "$(cat docs/examples/claude_code_mcp.json | jq '.mcpServers.syncmind')"
claude mcp list      # verify "syncmind" is listed
```

## 7. Ask Claude Code to use it

Open a new Claude Code session in any directory and ask:

> "Search my local notes for anything I wrote about embedding dimensions."

Claude Code will invoke `search_knowledge` and weave the results into its reply.

## 8. Verify live re-indexing

```bash
echo "## Side note on dimension mismatches" >> "$HOME/notes/research.md"
sleep 3     # the watcher has a 1-second debounce
syncmind search "dimension mismatch"
```

The new content should appear in the results.

## 9. Verify delete cleanup

```bash
rm "$HOME/code/myproject/README.md"
sleep 3
syncmind search "..." # the README content no longer appears
syncmind unregister "$HOME/code/myproject/README.md"   # also cleans up if you forgot to delete from index
```

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| `Failed to load ONNX model` | Check `<data-dir>/syncmind/models/` — the auto-download may have failed; check `syncmind.log.*` for the HTTP error and either retry or set `onnx_model_url` to a mirror |
| `Ollama probe returned HTTP 404` | The model name in config doesn't match what `ollama list` shows; either `ollama pull <model>` or change `ollama_model` |
| `No results found` | Run `syncmind status` — is the file actually indexed? Check the daemon's stderr/log for indexing errors |
| Claude Code shows "no tool available" | Run `claude mcp list`; confirm syncmind is listed and the binary path is correct |
| Daemon logs nothing in background mode | Check that `log_to_file = true` and that `<data-dir>/syncmind/logs/` is writable |
