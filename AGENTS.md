# AGENTS.md — sheets_mcp

Instructions for AI coding agents working on this codebase.
See `.plan/` for the full phased implementation plan.

## Project Overview

A high-performance MCP (Model Context Protocol) server for reading and writing Excel spreadsheets, built in Rust. Communicates via stdio JSON-RPC with LLM clients.

**5 MCP tools**: `read_structure`, `count_sheet_rows`, `get_sheet_range`, `update_cell`, `update_cells`

## Architecture

```
LLM Client (stdio) → rmcp server → Tool handlers → reader (calamine/umya) / writer (umya)
```

Two read paths:
- **Fast path (calamine)**: Values only, ~1M cells/sec. Used by `count_sheet_rows` and `get_sheet_range` (no format).
- **Format path (umya-spreadsheet)**: Values + formatting. Used by `read_structure` and `get_sheet_range` (with format).

One write path:
- **umya-spreadsheet**: Read-modify-write for `.xlsx`. Preserves formulas and formatting by design (value and style are independent XML elements).

## Crate Choices & Rationale

| Crate | Why |
|-------|-----|
| `rmcp` (v0.4) | Most actively maintained Rust MCP SDK. Features: `server`, `transport-stream`. |
| `calamine` (v0.26) | Pure Rust, no C deps. Fastest Excel reader (~1M cells/sec). Read-only. |
| `umya-spreadsheet` (v2.2) | Pure Rust read/write. Separate value/formula/style properties → safe updates. |
| `reqwest` (v0.12) | Async HTTP for URL downloads. Feature: `rustls-tls` (no OpenSSL dep, smaller binary). |
| `tokio` (v1) | Async runtime. Full features for simplicity. |
| `serde` + `serde_json` | JSON-RPC serialization. |
| `thiserror` + `anyhow` | Structured error types. |
| `tracing` + `tracing-subscriber` | Diagnostics to stderr (not stdout — stdio transport). |

## File Structure

```
src/
├── main.rs          # Binary entry: tokio::main, tracing init, rmcp serve
├── lib.rs           # Re-exports all modules
├── error.rs         # SheetsError enum (thiserror)
├── types.rs         # CellCoordinate, CellRange, CellValue, CellFormat, tool I/O structs
├── file_source.rs   # FileSource enum (Path/Url), resolve(), write_target(), download+cache
├── reader.rs        # read_structure(), count_sheet_rows(), get_sheet_range()
├── writer.rs        # update_cell(), update_cells()
└── tools.rs         # #[tool] handlers for rmcp, Serde input structs
```

## Module Responsibilities

### `error.rs`
- `SheetsError`: FileNotFound, SheetNotFound, InvalidCoordinate, Io, Http, Spreadsheet, MissingOutputPath
- Implements `From<std::io::Error>`

### `types.rs`
- `CellCoordinate`: Column (string + zero-based index) + Row (one-based). Implements `FromStr` and `Display`. Also a `from_index(col, row)` constructor.
- `CellRange`: Optional sheet name + start/end coordinates. Implements `FromStr` for `"A1:B10"` and `"Sheet1!A1:D100"`.
- `CellValue` enum: Empty, Bool(bool), Number(f64), String(String), Error(String). Derives `Serialize`.
- `CellFormat`: background_color, font_color, bold, italic, font_size, number_format, horizontal_alignment, has_border. All optional except bold/italic/border. Derives `Serialize`.
- `CellWithFormat`: coordinate + CellValue + CellFormat.
- Tool I/O structs: All derive `Deserialize` (input) or `Serialize` (output).

### `file_source.rs`
- `FileSource` enum with `#[serde(untagged)]`: `Url { url }` | `Path { file_path }`
- `resolve() -> Result<PathBuf>`: Async. Returns path directly for local; downloads+caches for URL.
- `write_target(output_path: Option<&str>) -> Result<PathBuf>`: For local sources, returns output_path or the original path. For URLs, requires output_path.
- Cache: `$TMPDIR/sheets_mcp/{hash}.{ext}`. Hash via DefaultHasher of URL. Preserves extension for format detection.
- Uses `reqwest::get()` with `error_for_status()`.

### `reader.rs`
- All functions are `async` (for FileSource::resolve).
- `read_structure(source, first_n_rows)`: Uses umya-spreadsheet. Returns sheets with dimensions + preview rows with CellWithFormat.
- `count_sheet_rows(source, sheet_name)`: Uses calamine. Fast metadata read.
- `get_sheet_range(source, sheet_name, range, include_format)`: Dispatches to calamine or umya based on `include_format`.

### `writer.rs`
- All functions are `async` (for FileSource::resolve + write_target).
- `update_cell(source, output_path, sheet_name, update)`: Read via umya, modify one cell, write back.
- `update_cells(source, output_path, sheet_name, destination, data)`: Parse destination coordinate, iterate 2D array with offset, set values, write back.
- Both preserve formulas (never touch `<f>` elements) and formatting (never touch `s=""` style index).

### `tools.rs`
- 5 tool functions with `#[tool(description = "...")]` attribute.
- Each takes a `#[derive(Deserialize)]` struct with `#[serde(flatten)] pub source: FileSource`.
- Returns `Result<String, String>` — JSON-serialized output or error string.
- A `create_server()` function that registers all tools with rmcp.

### `main.rs`
- `#[tokio::main]` entry point.
- `tracing_subscriber::fmt().with_writer(std::io::stderr).init()` — CRITICAL: logs to stderr.
- `rmcp::server::serve(tools::create_server()).transport(stream::stdio()).serve().await`

## Coding Conventions

- **Edition**: `2021` (NOT 2024 — umya-spreadsheet compatibility)
- **Error handling**: Use `SheetsError` for all fallible operations. Use `?` for propagation. Convert to `String` at the tool boundary (rmcp expects `Result<String, String>`).
- **Async**: All reader/writer functions are `async` to support `FileSource::resolve()` which downloads URLs.
- **Serde**: Input structs derive `Deserialize`. Output structs derive `Serialize`. Use `#[serde(rename_all = "snake_case")]` or explicit `#[serde(rename)]` as needed.
- **Color conversion**: umya-spreadsheet ARGB `u32` → hex `"#RRGGBB"`. Strip alpha byte.
- **Coordinate conversion**: Column index ↔ letter (A=0, B=1, ..., Z=25, AA=26, ...). Both directions needed.

## Testing

- **Fixtures**: Create `.xlsx` files programmatically in tests (using umya-spreadsheet to create, then test read/write against them).
- **Formula safety test**: Create cells with `=A1+B1`, update A1, verify formula intact in C1.
- **Format safety test**: Create styled cells, update values, verify styles unchanged.
- **Coordinate parsing**: Test edge cases: `A1`, `Z100`, `AA1`, `ZZ99`, empty string, `1A`.
- **Integration**: create → write → read → verify round-trips.

## Build & Run

```bash
# Development
cargo build
cargo run

# Release (for MCP client config)
cargo build --release

# Tests
cargo test

# Lint
cargo clippy

# Docs
cargo doc --open
```

## Formula & Format Safety — The Core Guarantee

This is the **most critical invariant** of the project. Never break it.

umya-spreadsheet stores each cell as independent XML elements:
- `<v>` — value
- `<f>` — formula (e.g., `=A1+B1`)
- `s=""` — style index (references a style definition with colors, fonts, etc.)

`cell.set_value("42")` only writes to `<v>`. It never touches `<f>` or `s=""`.

**Therefore**: updating any cell preserves ALL formulas and ALL formatting in the entire workbook. This is by design, not by accident. When writing new code in `writer.rs`, never use methods that reset or clear styles or formulas.

## File Input Model

Every tool accepts `file_path` OR `url` via `#[serde(flatten)]` on `FileSource`. They are mutually exclusive — provide one or the other, never both.

URL handling:
1. Downloads are cached in `$TMPDIR/sheets_mcp/` by URL hash
2. Cache preserves file extension for format detection
3. No TTL — delete manually to force refresh
4. Write tools require `output_path` when source is a URL
5. Write tools write back to same path when source is a local file_path

## Known Gotchas

- **calamine is read-only**: Don't try to write with it. Use umya-spreadsheet for all writes.
- **umya-spreadsheet loads full DOM**: For 100MB+ files, memory usage scales proportionally.
- **rmcp logging**: Must go to stderr. Stdout is the MCP transport channel.
- **umya-spreadsheet API**: Cell access is via `sheet.get_cell("A1")` or `sheet.get_cell((col_idx, row_idx))`. Check the actual API at implementation time.
- **reqwest TLS**: Using `rustls-tls` to avoid linking OpenSSL. If download fails on some platforms, consider `native-tls` as fallback.