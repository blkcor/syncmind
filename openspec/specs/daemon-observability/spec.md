# daemon-observability Specification

## Purpose
Provide persistent, rotating log files for the SyncMind daemon so that issues encountered in background mode can be diagnosed without re-running with `--foreground`, while preserving stdout discipline for the Stdio MCP transport.

## Requirements

### Requirement: Always-on file logging
The daemon SHALL initialize tracing such that logs are always written to a rolling file under `~/.local/share/syncmind/logs/`, regardless of whether `--foreground` is set.

#### Scenario: Background daemon writes to disk
- **WHEN** `syncmind daemon` is started without `--foreground`
- **THEN** a file matching `~/.local/share/syncmind/logs/syncmind.log.<YYYY-MM-DD>` exists after the first log event
- **AND** the file contains the daemon's startup messages

#### Scenario: Foreground daemon additionally writes to stderr
- **WHEN** `syncmind daemon --foreground` is started
- **THEN** logs appear on stderr in addition to the rolling file

### Requirement: Stdio MCP transport stdout safety
The daemon SHALL NOT write log output to stdout, ensuring that the Stdio MCP transport's JSON-RPC stream remains uncorrupted.

#### Scenario: Stdio mode keeps stdout exclusively for JSON-RPC
- **WHEN** the daemon runs with `mcp_transport = "stdio"`
- **THEN** stdout contains only valid JSON-RPC frames (one JSON object per line)
- **AND** no `tracing` output appears on stdout

### Requirement: Configurable log behavior
The system SHALL expose `log_level`, `log_to_file`, and `log_rotation` fields in `config.toml`, with sensible defaults that work out-of-the-box.

#### Scenario: Default config produces info-level daily rotation
- **WHEN** `config.toml` is created from defaults
- **THEN** `log_level = "info"`
- **AND** `log_to_file = true`
- **AND** `log_rotation = "daily"`

#### Scenario: log_to_file = false disables the file appender
- **WHEN** `config.toml` sets `log_to_file = false`
- **THEN** no log file is created
- **AND** logs still appear on stderr if `--foreground` is set

### Requirement: Graceful fallback when log directory is not writable
The system SHALL not crash when `~/.local/share/syncmind/logs/` cannot be created or written; instead it SHALL fall back to stderr-only logging and emit a single error describing the cause.

#### Scenario: Read-only log directory
- **WHEN** the log directory's parent is read-only or otherwise inaccessible
- **THEN** the daemon emits a `tracing::error!` describing the failure (visible on stderr)
- **AND** continues to run with stderr-only logging
