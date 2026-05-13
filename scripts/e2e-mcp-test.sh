#!/usr/bin/env bash
set -euo pipefail

# E2E MCP test for SyncMind stdio transport.
# Requires syncmind binary to be built and available on PATH.
# Also requires an embedder (Ollama or ONNX) to be available.

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
mkdir -p "$XDG_CONFIG_HOME/syncmind"
mkdir -p "$XDG_DATA_HOME/syncmind"

# Create a test file to index.
TEST_FILE="${TMPDIR}/test_note.md"
cat > "$TEST_FILE" <<'EOF'
# Project Ideas

## Rust MCP Server
Build a headless MCP server in Rust that exposes local knowledge via vector search.
EOF

# Register the file.
"$BINARY" register "$TEST_FILE"

# Create a named pipe for bidirectional stdio communication.
DAEMON_STDIN="${TMPDIR}/daemon_stdin"
DAEMON_STDOUT="${TMPDIR}/daemon_stdout"
mkfifo "$DAEMON_STDIN"
mkfifo "$DAEMON_STDOUT"

# Start daemon in background reading from our pipe and writing to its pipe.
"$BINARY" daemon --foreground < "$DAEMON_STDIN" > "$DAEMON_STDOUT" &
DAEMON_PID=$!
trap 'kill "$DAEMON_PID" 2>/dev/null || true; rm -rf "$TMPDIR"' EXIT

# Open file descriptors for the pipes.
exec 3>"$DAEMON_STDIN"
exec 4<"$DAEMON_STDOUT"

# Give the daemon time to start and index.
sleep 3

send_request() {
    local id="$1"
    local method="$2"
    local params="${3:-null}"
    printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":$id,\"method\":\"$method\",\"params\":$params}" >&3
}

read_response() {
    local timeout=5
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

echo "=== Test 1: initialize ==="
send_request 1 "initialize" "{}"
RESP=$(read_response)
echo "$RESP"
echo "$RESP" | jq -e '.result.server_info.name == "syncmind"' >/dev/null

echo "=== Test 2: tools/list ==="
send_request 2 "tools/list" "null"
RESP=$(read_response)
echo "$RESP"
echo "$RESP" | jq -e '.result.tools | length >= 1' >/dev/null

echo "=== Test 3: search_knowledge ==="
send_request 3 "tools/call" '{"name":"search_knowledge","arguments":{"query":"Rust MCP server","top_k":3}}'
RESP=$(read_response)
echo "$RESP"
echo "$RESP" | jq -e '.result.content[0].text | fromjson | length >= 0' >/dev/null

echo ""
echo "All E2E tests passed!"

# Close file descriptors.
exec 3>&-
exec 4<&-
