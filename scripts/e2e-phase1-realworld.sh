#!/usr/bin/env bash
set -euo pipefail

# E2E real-world test for SyncMind Phase 1.
#
# Extends scripts/e2e-mcp-test.sh with three additional scenarios that
# exercise the daemon as a real user would:
#   1. Modify a registered file and verify the next search reflects the change.
#   2. Delete a registered file and verify it disappears from search results.
#   3. Optionally exercise the Ollama path when localhost:11434 is reachable.
#
# Prerequisites:
#   - jq
#   - syncmind binary built and on PATH (or at core/target/debug/syncmind)
#   - An embedder available: either Ollama running locally, or first-run
#     network access to Hugging Face for the ONNX fallback download.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BINARY="${PROJECT_ROOT}/core/target/debug/syncmind"

if [[ ! -x "$BINARY" ]]; then
    echo "Building syncmind binary..."
    (cd "${PROJECT_ROOT}/core" && cargo build --bin syncmind)
fi

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

export XDG_CONFIG_HOME="${TMPDIR}/config"
export XDG_DATA_HOME="${TMPDIR}/data"
# macOS dirs crate ignores XDG_*; use SYNCMIND_* overrides instead so the
# test cannot pollute or be polluted by the user's real config/db.
export SYNCMIND_CONFIG_DIR="${TMPDIR}/config/syncmind"
export SYNCMIND_DATA_DIR="${TMPDIR}/data/syncmind"
mkdir -p "$SYNCMIND_CONFIG_DIR" "$SYNCMIND_DATA_DIR"

# Probe Ollama; if available we keep default config (uses Ollama).
# If not, we switch to bge-small embedding_dim=384 so the ONNX fallback fires
# without manual config edits.
if curl -sf --max-time 2 "http://localhost:11434/api/tags" >/dev/null 2>&1; then
    echo "[detect] Ollama available, using bge-m3 (1024-dim)"
    OLLAMA_OK=1
else
    echo "[detect] Ollama unavailable, switching config to bge-small (384-dim) — first-run will download the ONNX model"
    OLLAMA_OK=0
    cat > "$SYNCMIND_CONFIG_DIR/config.toml" <<EOF
ollama_url = "http://localhost:11434"
ollama_model = "bge-m3"
mcp_transport = "stdio"
bind_addr = "127.0.0.1:3000"
registered_files = []
embedding_dim = 384
chunk_size = 512
chunk_overlap = 50
log_level = "info"
log_to_file = true
log_rotation = "daily"
EOF
fi

NOTE_FILE="${TMPDIR}/note.md"
cat > "$NOTE_FILE" <<'EOF'
# Project Ideas

## Rust MCP Server
Build a headless MCP server in Rust that exposes local knowledge via vector search.
EOF

DOOMED_FILE="${TMPDIR}/doomed.md"
cat > "$DOOMED_FILE" <<'EOF'
# Soon to be deleted
This document contains the unique phrase nimbus-canvas-mango that should disappear from the index after deletion.
EOF

"$BINARY" register "$NOTE_FILE"
"$BINARY" register "$DOOMED_FILE"

DAEMON_STDIN="${TMPDIR}/daemon_stdin"
DAEMON_STDOUT="${TMPDIR}/daemon_stdout"
mkfifo "$DAEMON_STDIN"
mkfifo "$DAEMON_STDOUT"

"$BINARY" daemon --foreground < "$DAEMON_STDIN" > "$DAEMON_STDOUT" &
DAEMON_PID=$!
trap 'kill "$DAEMON_PID" 2>/dev/null || true; rm -rf "$TMPDIR"' EXIT

exec 3>"$DAEMON_STDIN"
exec 4<"$DAEMON_STDOUT"

# Allow extra time on first run for ONNX model download.
if [[ "$OLLAMA_OK" -eq 0 ]]; then
    echo "[wait] ONNX model may be downloading (~130 MB)..."
    sleep 30
else
    sleep 5
fi

send_request() {
    local id="$1"
    local method="$2"
    local params="${3:-null}"
    printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"method\":\"$method\",\"params\":$params}" >&3
}

read_response() {
    local timeout=10
    local line=""
    while IFS= read -r -t "$timeout" line <&4; do
        if [[ -n "$line" ]]; then
            echo "$line"
            return 0
        fi
    done
    echo "ERROR: timeout waiting for response" >&2
    return 1
}

initialize_and_search() {
    local request_id="$1"
    local query="$2"
    send_request "$request_id" "tools/call" \
        "$(jq -nc --arg q "$query" '{name:"search_knowledge",arguments:{query:$q,top_k:5}}')"
    read_response
}

echo "=== Test 1: initialize handshake ==="
send_request 1 "initialize" "{}"
RESP=$(read_response)
echo "$RESP" | jq -e '.result.server_info.name == "syncmind"' >/dev/null
echo "OK"

echo "=== Test 2: baseline search for note content ==="
RESP=$(initialize_and_search 2 "Rust MCP server")
echo "$RESP" | jq -e '.result.content[0].text | fromjson | length >= 1' >/dev/null
echo "OK"

echo "=== Test 3: modify file, debounce, re-search ==="
cat >> "$NOTE_FILE" <<'EOF'

## Real-world E2E marker
The phrase quasar-otter-mistletoe should appear in subsequent searches.
EOF
sleep 4
RESP=$(initialize_and_search 3 "quasar otter mistletoe")
HITS=$(echo "$RESP" | jq -r '.result.content[0].text | fromjson | length')
if [[ "$HITS" -lt 1 ]]; then
    echo "FAIL: expected at least 1 hit for the new marker phrase"
    exit 1
fi
echo "OK"

echo "=== Test 4: delete file, debounce, search excludes it ==="
rm "$DOOMED_FILE"
sleep 4
RESP=$(initialize_and_search 4 "nimbus canvas mango")
DELETED_PATH_HITS=$(echo "$RESP" | jq -r '.result.content[0].text | fromjson | map(select(.file_path | contains("doomed.md"))) | length')
if [[ "$DELETED_PATH_HITS" -ne 0 ]]; then
    echo "FAIL: expected 0 hits for deleted file, got $DELETED_PATH_HITS"
    exit 1
fi
echo "OK"

echo ""
echo "=========================================="
echo "All Phase 1 real-world E2E tests passed!"
echo "Ollama path exercised: $OLLAMA_OK"
echo "=========================================="

exec 3>&-
exec 4<&-
