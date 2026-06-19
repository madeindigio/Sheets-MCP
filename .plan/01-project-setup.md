# Phase 0: Project Setup & Dependencies

## Goal
Set up the Rust project with all dependencies, create the crate structure (lib + bin), and verify everything compiles.

## Steps

### 1. Update `Cargo.toml`

```toml
[package]
name = "sheets_mcp"
version = "0.1.0"
edition = "2021"  # Note: umya-spreadsheet needs 2021, not 2024

[dependencies]
# MCP SDK
rmcp = { version = "0.4", features = ["server", "transport-stream"] }

# Async
tokio = { version = "1", features = ["full"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Excel / Spreadsheet
calamine = "0.26"
umya-spreadsheet = "2.2"

# HTTP (for URL downloads)
reqwest = { version = "0.12", features = ["rustls-tls"], default-features = false }

# Error handling
thiserror = "2"
anyhow = "1"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"
```

### 2. Create Crate Structure

```
src/
├── main.rs          # Binary entry point
├── lib.rs           # Library root, re-exports
├── error.rs         # Error types
├── types.rs         # Domain models, coordinate parsing
├── file_source.rs   # Resolves file_path/url → local path (with download + cache)
├── reader.rs        # calamine + umya-spreadsheet read operations
├── writer.rs        # umya-spreadsheet write operations
└── tools.rs         # MCP tool handler definitions
```

### 3. Verify

```bash
cargo build
```

## Key Decisions
- **Edition 2021** (not 2024): `umya-spreadsheet` has known issues with edition 2024 resolver
- **`rmcp` features**: `server` for MCP server, `transport-stream` for stdio transport
- **`tokio` features**: `full` for simplicity (can optimize later with individual features)