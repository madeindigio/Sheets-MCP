# 📋 Sheets MCP — Master Plan

## Architecture

```
┌─────────────┐     stdio/JSON-RPC      ┌──────────────────┐
│  LLM / MCP  │ ◄──────────────────────► │  sheets_mcp      │
│  Client     │                          │  (Rust binary)   │
└─────────────┘                          │                  │
                                         │ ┌──────────────┐ │
                                         │ │ rmcp server  │ │
                                         │ └──────┬───────┘ │
                                         │        │          │
                                         │ ┌──────▼───────┐ │
                                         │ │ Tool handlers│ │
                                         │ └──────┬───────┘ │
                                         │        │          │
                                         │ ┌──────▼───────┐ │
                                         │ │  File Input  │ │
                                         │ │ path → disk  │ │
                                         │ │ url  → GET   │ │
                                         │ │  + cache     │ │
                                         │ └──────┬───────┘ │
                                         │        │          │
                                         │ ┌──────▼───────┐ │
                                         │ │  Read path   │ │
                                         │ │ calamine     │ │
                                         │ │ (fast, values│ │
                                         │ │  only)       │ │
                                         │ ├──────────────┤ │
                                         │ │  Read+Format │ │
                                         │ │ umya-sprdsht │ │
                                         │ │ (values+fmt) │ │
                                         │ ├──────────────┤ │
                                         │ │  Write path  │ │
                                         │ │ (umya)       │ │
                                         │ └──────────────┘ │
                                         └──────────────────┘
```

## File Input Model

Every tool accepts either a **local file path** or a **URL**.

| Scenario | Input | Read | Write |
|----------|-------|------|-------|
| Local file | `file_path: "/home/user/report.xlsx"` | ✅ Reads from disk | ✅ Writes back to same path |
| Remote file | `url: "https://example.com/data.xlsx"` | ✅ Downloads, caches in temp dir | ❌ N/A — provide `output_path` |
| Remote + write | `url: "..."` + `output_path: "/home/user/output.xlsx"` | ✅ Downloads | ✅ Writes to `output_path` |

### How it works

1. **Tool receives `file_path` OR `url`** (not both) — internally resolved to a local path
2. **URL downloads** are cached in `$TMPDIR/sheets_mcp/` by URL hash — repeated reads of the same URL are instant
3. **Write tools** (`update_cell`, `update_cells`) require an `output_path` when the source was a URL. If the source was a local `file_path`, writes go back to the same path by default.
4. **No file content is ever sent through the LLM** — only paths/URLs, coordinates, and values. LLM token usage is minimal.

### Token efficiency

The LLM sees path strings like `"https://..."` or `"/home/user/file.xlsx"` — that's it. The MCP binary handles downloading, reading, and writing. Returning 10 preview rows from `read_structure` is bounded and cheap.

## Tech Stack

| Component | Crate | Purpose |
|-----------|-------|---------|
| MCP SDK | `rmcp` | MCP protocol, tool macros, stdio transport |
| Async runtime | `tokio` | Async I/O, multi-threading |
| Serialization | `serde` + `serde_json` | JSON-RPC messages |
| Read .xlsx/.xls (values only) | `calamine` | High-speed read-only value inspection |
| Read .xlsx (values + formatting) | `umya-spreadsheet` | Reads colors, fonts, borders, styles |
| Write .xlsx | `umya-spreadsheet` | Read-modify-write, preserves formulas & formatting |
| Error handling | `thiserror` + `anyhow` | Structured errors |
| Logging | `tracing` | Diagnostics |

## Supported File Formats

- ✅ `.xlsx` — Read (values + formatting), update (umya-spreadsheet)
- ✅ `.xls` — Read-only (calamine, values only, no formatting)

### Formula Safety Guarantee
When updating a cell value, **all other cells are untouched**. Formulas like `=A1+B1` in cell C1 will recalculate correctly when the workbook is reopened in Excel.

### Format Preservation Guarantee
When updating a cell value, **existing formatting is always preserved**. Background colors, fonts, borders, and number formats remain unchanged. This is achieved because `umya-spreadsheet` stores styles as separate objects referenced by index — `set_value()` only modifies the value XML element, never the style reference.

## Phases

| # | Phase | Description |
|---|-------|-------------|
| 0 | [Project Setup](./01-project-setup.md) | Cargo.toml, dependencies, crate structure |
| 0.5 | [File Source Resolution](./01.5-file-source.md) | Dual file_path/url input, download + cache |
| 1 | [Domain Models](./02-domain-models.md) | Shared types, errors, coordinate parsing |
| 2 | [Read Operations](./03-read-operations.md) | `read_structure`, `count_sheet_rows`, `get_sheet_range` |
| 3 | [Write Operations](./04-write-operations.md) | `update_cell`, `update_cells` (array paste) |
| 4 | [MCP Server Wiring](./05-mcp-server.md) | rmcp integration, tool registration, main loop |
| 5 | [Testing & Polish](./06-testing-polish.md) | Tests, error handling, docs |
| 6 | [Documentation](./07-documentation.md) | README, MCP client config, usage examples |

## MCP Tools (5 total)

| # | Tool | Description |
|---|------|-------------|
| 1 | `read_structure` | Returns sheet names, column count, row count, and the first N rows with their **values and formatting** (colors, fonts, borders). The LLM uses this to understand the spreadsheet layout and identify "the gray cells". |
| 2 | `count_sheet_rows` | Returns the total number of rows in a sheet (fast, calamine-based). |
| 3 | `get_sheet_range` | Returns cell values (and optionally formatting) from a given range like `A1:D100`. |
| 4 | `update_cell` | Updates a single cell's value. Preserves formulas and formatting in all cells. |
| 5 | `update_cells` | Pastes a 2D array of values starting at a destination cell (e.g., `B2`). Preserves formulas and formatting in all cells. |