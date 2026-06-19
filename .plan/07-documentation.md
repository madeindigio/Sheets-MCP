# Phase 6: Documentation

## Goal
Create comprehensive documentation so users can install, configure, and use the MCP server with any MCP client (VS Code, Claude Desktop, etc.).

---

## Files to Create

### 0. `AGENTS.md` (repository root)

Provides instructions for AI coding agents working on this codebase. See the file in the repo root for the full content.

Key sections:
- Project overview and architecture
- Crate choices and why (calamine vs umya-spreadsheet, rmcp, reqwest)
- File structure and module responsibilities
- Coding conventions (edition 2021, error handling with thiserror, async with tokio)
- Testing approach (fixtures, formula safety tests, format preservation tests)
- Build and run commands

### 1. `README.md` (repository root)

```markdown
# 📊 sheets_mcp

A high-performance MCP server for reading and writing Excel spreadsheets, built in Rust.

## Features

- **5 MCP tools** — `read_structure`, `count_sheet_rows`, `get_sheet_range`, `update_cell`, `update_cells`
- **Local files & URLs** — pass a file_path or a URL, the MCP handles the rest
- **Formatting-aware** — read cell colors, fonts, borders; write without destroying them
- **Formula-safe** — updating cell values never touches formulas in other cells
- **Blazing fast** — calamine reads ~1M cells/sec; sub-10MB memory footprint

## Supported Formats

| Format | Read | Write | Formatting |
|--------|------|-------|------------|
| `.xlsx` | ✅ | ✅ | ✅ |
| `.xls` | ✅ (values only) | ❌ | ❌ |

## Installation

### From source (requires Rust)

```bash
git clone <repo-url>
cd sheets_mcp
cargo build --release
```

Binary at: `./target/release/sheets_mcp`

### From crates.io (future)

```bash
cargo install sheets_mcp
```

## MCP Client Configuration

Add to your MCP client config:

### VS Code / GitHub Copilot

```json
{
  "mcpServers": {
    "sheets": {
      "command": "/path/to/sheets_mcp/target/release/sheets_mcp"
    }
  }
}
```

### Claude Desktop

```json
{
  "mcpServers": {
    "sheets": {
      "command": "/path/to/sheets_mcp/target/release/sheets_mcp"
    }
  }
}
```

## Tools Reference

### 1. `read_structure`

Returns sheet names, dimensions, and the first N rows with **values and formatting**.

```
Input:
  file_path: "/home/user/report.xlsx"   (or: url: "https://...")
  first_n_rows: 10                       (default: 10)

Output:
{
  "file_path": "/home/user/report.xlsx",
  "sheets": [
    {
      "name": "Sheet1",
      "total_rows": 150,
      "total_columns": 8,
      "preview_rows": [
        [
          {
            "coordinate": "A1",
            "value": { "String": "Name" },
            "format": {
              "background_color": "#4472C4",
              "font_color": "#FFFFFF",
              "bold": true,
              "italic": false,
              "font_size": 11.0,
              "number_format": null,
              "horizontal_alignment": "Center",
              "has_border": false
            }
          },
          ...
        ]
      ]
    }
  ]
}
```

**Use this when**: You need to understand the spreadsheet layout, find "the gray cells", or discover column headers.

---

### 2. `count_sheet_rows`

Fast row/column count for a sheet.

```
Input:
  file_path: "/home/user/report.xlsx"   (or: url: "https://...")
  sheet_name: "Sheet1"

Output:
{
  "sheet_name": "Sheet1",
  "total_rows": 150,
  "total_columns": 8
}
```

**Use this when**: You need to know how many rows exist before requesting a range.

---

### 3. `get_sheet_range`

Get cell values from a range. Optionally includes formatting.

```
Input:
  file_path: "/home/user/report.xlsx"   (or: url: "https://...")
  sheet_name: "Sheet1"
  range: "A1:D100"
  include_format: false                  (default: false, for speed)

Output (include_format: false):
{
  "sheet_name": "Sheet1",
  "range": "A1:D100",
  "rows": [
    [
      { "value": { "String": "Name" } },
      { "value": { "String": "Age" } },
      ...
    ]
  ]
}

Output (include_format: true):
{
  "sheet_name": "Sheet1",
  "range": "A1:D100",
  "rows": [
    [
      {
        "value": { "String": "Name" },
        "format": {
          "background_color": "#4472C4",
          "font_color": "#FFFFFF",
          "bold": true,
          ...
        }
      },
      ...
    ]
  ]
}
```

**Use this when**: You need a specific subset of cells. Use `include_format: false` (fast) for values-only reads. Use `include_format: true` only when you need colors/fonts.

---

### 4. `update_cell`

Update a single cell's value. **Preserves all formulas and formatting**.

```
Input:
  file_path: "/home/user/report.xlsx"   (or: url: "https://...")
  sheet_name: "Sheet1"
  coordinate: "B5"
  value: "42"
  value_type: "Number"                   ("String", "Number", or "Bool")
  output_path: "/home/user/output.xlsx"  (optional; required if source is a URL)

Output:
{
  "updated_count": 1,
  "sheet_name": "Sheet1",
  "coordinate": "B5",
  "output_path": "/home/user/report.xlsx"
}
```

**Guarantees**:
- Other cells with formulas like `=B5*2` will recalculate correctly
- Cell formatting (colors, fonts, borders) is untouched
- Only the value changes

---

### 5. `update_cells`

Paste a 2D array of values starting at a destination cell.

```
Input:
  file_path: "/home/user/report.xlsx"   (or: url: "https://...")
  sheet_name: "Sheet1"
  destination: "B2"                      (top-left corner of paste)
  data: [
    ["Alice", "30", "Engineer"],
    ["Bob", "25", "Designer"]
  ]
  output_path: "/home/user/output.xlsx"  (optional; required if source is a URL)

Output:
{
  "updated_count": 6,
  "sheet_name": "Sheet1",
  "start": "B2",
  "rows": 2,
  "columns": 3,
  "output_path": "/home/user/report.xlsx"
}
```

Paste result:
```
Before:           After:
   A  B  C  D        A  B       C         D
1  .  .  .  .     1  .  .       .         .
2  .  .  .  .     2  .  Alice   30        Engineer
3  .  .  .  .     3  .  Bob     25        Designer
```

**Same guarantees as `update_cell`** — formulas and formatting preserved everywhere.

---

## File Input: `file_path` vs `url`

Every tool accepts **either** a local path or a remote URL:

| Usage | Example |
|-------|---------|
| Local read | `{ "file_path": "/home/user/data.xlsx" }` |
| Remote read | `{ "url": "https://cdn.example.com/data.xlsx" }` |
| Local write | `{ "file_path": "/home/user/data.xlsx", ... }` (writes back in-place) |
| Remote write | `{ "url": "...", "output_path": "/home/user/modified.xlsx", ... }` |

URLs are downloaded and cached in `$TMPDIR/sheets_mcp/`. Repeated reads are instant.

## Common Workflows

### "Update the gray cells"

```
1. read_structure("report.xlsx", first_n_rows: 20)
   → discovers: A4..A10 have background_color: "#D9D9D9" (gray)

2. update_cells("report.xlsx", "Sheet1", "A4", [["42"],["99"],["37"],...])
   → values updated, gray formatting preserved
```

### "Add data to the end of a sheet"

```
1. count_sheet_rows("data.xlsx", "Sheet1")
   → "total_rows": 150

2. update_cells("data.xlsx", "Sheet1", "A151", [["new","row","data"]])
   → appends after existing data
```

### "Download and modify a template"

```
1. read_structure({ url: "https://intranet/template.xlsx" }, first_n_rows: 5)
   → reads template structure

2. update_cells(
     { url: "https://intranet/template.xlsx" },
     "Sheet1", "B2",
     [["Q1", "100"], ["Q2", "200"]],
     output_path: "/home/user/filled.xlsx"
   )
   → fills template, saves locally
```

## Performance

| Operation | Throughput | Notes |
|-----------|-----------|-------|
| `read_structure` (10 rows) | ~5ms | umya-spreadsheet DOM load |
| `count_sheet_rows` | <1ms | calamine metadata read |
| `get_sheet_range` (no format) | ~1M cells/sec | calamine streaming |
| `get_sheet_range` (with format) | ~10K cells/sec | umya-spreadsheet DOM |
| `update_cell` | ~10ms | DOM load + save |
| `update_cells` | ~10ms + ~1μs/cell | DOM load + save |

Memory footprint: ~8-12MB idle.

## Known Limitations

- `.xls` files: read-only, no formatting support
- `.xlsx` files with macros (`.xlsm`): not supported
- Very large files (>100MB): umya-spreadsheet loads entire DOM into memory — use `count_sheet_rows` first to check size
- URL cache has no TTL — delete `$TMPDIR/sheets_mcp/` manually to force re-download
- No chart/image preservation testing (umya-spreadsheet handles them but verify)

## License

MIT
```

---

### 2. Doc comments on every public API

Add `///` documentation to every public struct, function, and enum in:

| File | Items to document |
|------|-------------------|
| `src/types.rs` | `CellCoordinate`, `CellRange`, `CellValue`, `CellFormat`, `CellWithFormat`, all I/O structs |
| `src/error.rs` | `SheetsError` enum variants |
| `src/file_source.rs` | `FileSource` enum, `resolve()`, `write_target()` |
| `src/reader.rs` | `read_structure()`, `count_sheet_rows()`, `get_sheet_range()` |
| `src/writer.rs` | `update_cell()`, `update_cells()` |

Example:
```rust
/// Updates a single cell's value in an Excel workbook.
///
/// # Guarantees
/// - Preserves all existing formulas in the workbook
/// - Preserves all existing cell formatting (colors, fonts, borders)
/// - Only the target cell's value XML element is modified
///
/// # Arguments
/// * `source` - Local file path or remote URL to the workbook
/// * `output_path` - Where to save (required for URLs, optional for local files)
/// * `sheet_name` - Name of the sheet containing the cell
/// * `update` - Cell coordinate, new value, and value type
pub fn update_cell(
    source: &FileSource,
    output_path: Option<&str>,
    sheet_name: &str,
    update: CellUpdate,
) -> Result<UpdateResult, SheetsError> {
    // ...
}
```

---

## Acceptance Criteria
- [ ] `README.md` exists with installation, config, and all 5 tool references
- [ ] Each tool section includes input/output examples in JSON
- [ ] Common workflows section shows real use cases
- [ ] File input (`file_path` vs `url`) is clearly documented with examples
- [ ] All public API items have `///` doc comments
- [ ] `cargo doc --open` produces navigable HTML docs
- [ ] Performance characteristics documented
- [ ] Known limitations documented