# 📊 sheets_mcp

A high-performance MCP server for reading and writing Excel spreadsheets, built in Rust.

## Features

- **7 MCP tools** — `read_structure`, `count_sheet_rows`, `get_sheet_range`, `update_cell`, `update_cells`, `insert_rows`, `read_template_metadata`
- **Local files & URLs** — pass a `file_path` or a `url`, the MCP handles the rest
- **Formatting-aware** — read cell colors, fonts, borders; write without destroying them
- **Formula-safe** — updating cell values never touches formulas in other cells
- **Blazing fast** — calamine reads ~1M cells/sec; sub-10MB memory footprint

## Supported Formats

| Format | Read | Write | Formatting |
|--------|------|-------|------------|
| `.xlsx` | ✅ | ✅ | ✅ |
| `.xls` | ✅ (values only) | ✅ (via LibreOffice round-trip, requires `soffice` on `PATH`) | ⚠️ best-effort |

Writing to legacy `.xls` files requires **LibreOffice** to be installed and
available as `soffice` on `PATH` (e.g. `apt install libreoffice-calc` or
`brew install --cask libreoffice`). The `.xls` is transparently converted to
`.xlsx`, edited, and converted back — no manual conversion is needed on the
client side. If `soffice` is missing, `update_cell`/`update_cells` return a
clear error instead of failing on `.xlsx`-only reads/writes.

## Installation

### From source (requires Rust)

```bash
git clone <repo-url>
cd sheets_mcp
cargo build --release
```

Binary at: `./target/release/sheets_mcp`

### Static Linux build (musl)

Works on any Linux distro regardless of glibc version (Alpine, old CentOS, distroless):

```bash
sudo apt install musl-tools musl-dev   # Debian/Ubuntu
rustup target add x86_64-unknown-linux-musl
CC_x86_64_unknown_linux_musl=musl-gcc cargo build --release --target x86_64-unknown-linux-musl
```

Binary at: `./target/x86_64-unknown-linux-musl/release/sheets_mcp`

Verify it is fully static:

```bash
file target/x86_64-unknown-linux-musl/release/sheets_mcp   # "statically linked"
ldd  target/x86_64-unknown-linux-musl/release/sheets_mcp   # "not a dynamic executable"
```

Prebuilt `sheets_mcp-x86_64-unknown-linux-musl.tar.gz` and
`sheets_mcp-aarch64-unknown-linux-musl.tar.gz` assets are published with each release.

Optional (only needed to write legacy `.xls` files):

```bash
sudo apt install libreoffice-calc   # Debian/Ubuntu
```

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

For `.xls` files, `read_structure` returns **values only**. The `format` field is present but contains default/empty formatting because legacy `.xls` formatting extraction is not supported.

```
Input:
  file_path: "/home/user/report.xlsx"   (or: url: "https://...")
  first_n_rows: 10                       (default: 10)

Output:
{
  "sheets": [
    {
      "name": "Sheet1",
      "total_rows": 150,
      "total_columns": 8,
      "preview_rows": [
        [
          {
            "coordinate": "A1",
            "value": { "type": "String", "value": "Name" },
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

Get cell values from a range. Optionally includes formatting and formula text.

For `.xls` files, `get_sheet_range` is **values-only**. Formatting and formula metadata are not supported for legacy `.xls` workbooks.

```
Input:
  file_path: "/home/user/report.xlsx"   (or: url: "https://...")
  sheet_name: "Sheet1"
  range: "A1:D100"
  include_format: false                  (default: false, for speed)
  include_formula: false                 (default: false, for speed)

Output (include_format: false):
{
  "sheet_name": "Sheet1",
  "range": "A1:D100",
  "formulas_included": false,
  "rows": [
    [
      { "value": { "type": "String", "value": "Name" } },
      { "value": { "type": "String", "value": "Age" } },
      ...
    ]
  ]
}

Output (include_format: true):
{
  "sheet_name": "Sheet1",
  "range": "A1:D100",
  "formulas_included": false,
  "rows": [
    [
      {
        "value": { "type": "String", "value": "Name" },
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

Output with `include_formula: true` additionally sets `formulas_included: true`
and includes a `formula` string (without a leading `=`) on formula cells. Formula
inspection uses the `.xlsx` reader even when `include_format` is false, so it is
slower than the values-only path. For `.xls`, `formulas_included` remains false.

**Use this when**: You need a specific subset of cells. Use `include_format: false` and `include_formula: false` (the defaults) for fast values-only reads. Enable either option only when needed.

---

### 4. `update_cell`

Update a single cell's value. **Preserves all formulas and formatting**.

```
Input:
  file_path: "/home/user/report.xlsx"   (or: url: "https://...")
  sheet_name: "Sheet1"
  coordinate: "B5"
  value: "42"
  value_type: "number"                   ("string", "number", or "bool")
  output_path: "/home/user/output.xlsx"  (optional; required if source is a URL)

Output:
{
  "updated_count": 1,
  "sheet_name": "Sheet1",
  "output_path": "/home/user/report.xlsx",
  "start": "B5",
  "rows": 1,
  "columns": 1
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
  "output_path": "/home/user/report.xlsx",
  "start": "B2",
  "rows": 2,
  "columns": 3
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

### 6. `insert_rows`

Insert rows after a row, giving each new row the format, height and formulas of a template row. Use it before `update_cells` when a template has fewer pre-formatted rows than the data.

```
Input:
  file_path: "/home/user/trips.xlsx"
  sheet_name: "Viajes y Dietas"
  after_row: 11            (new rows start at 12)
  count: 12
  copy_from_row: 11        (optional; defaults to after_row, must not be below it)
  output_path: "..."       (optional; required if source is a URL)

Output:
{
  "sheet_name": "Viajes y Dietas",
  "output_path": "/home/user/trips.xlsx",
  "inserted_rows": 12,
  "first_new_row": 12,
  "last_new_row": 23,
  "cells_copied": 324,
  "formulas_copied": 60
}
```

- Formulas are copied with relative row references shifted to the new row (`O11*0.19` → `O12*0.19`); absolute rows (`$7`), text and quoted sheet names are kept.
- Everything below the insertion point (cells, merged ranges, drawings such as note text boxes, comments, conditional formats and references to it) moves down, as when rows are inserted in Excel.
- `.xls` files use the same LibreOffice round trip as `update_cells`.

---

### 7. `read_template_metadata`

Return what cell reads do not show in a template: the text of drawing text boxes (column notes printed in a shape), cell comments and merged ranges.

```
Input:
  file_path: "/home/user/template.xlsx"
  sheet_name: "Viajes y Dietas"   (optional; all sheets when omitted)

Output:
{
  "sheets": [{
    "sheet_name": "Viajes y Dietas",
    "notes_available": true,
    "drawing_notes": [{ "anchor": "A13",
                        "paragraphs": ["(3) Tipo de gasto: Debe elegir entre las opciones:", " · Viajes y dietas ", "..."] }],
    "comments": [{ "cell": "C5", "text": "..." }],
    "merged": ["A5:A6", "N4:Z4"]
  }]
}
```

- One entry per text box, with its paragraphs in order (empty paragraphs kept, so callers can split numbered notes); `anchor` is the shape's top-left cell, `null` for absolute anchors.
- Notes are read from the `.xlsx` parts directly, so grouped shapes and plain-text comments are included.
- Legacy `.xls` files return their sheet names with `notes_available: false`.

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
| `update_cell`/`update_cells` on `.xls` | ~1-2s | LibreOffice headless round-trip |

Memory footprint: ~8-12MB idle.

## Known Limitations

- `.xls` files: no formatting support on read; writes round-trip through LibreOffice (`soffice` required on `PATH`) and may occasionally alter styling
- `.xlsx` files with macros (`.xlsm`): not supported
- Very large files (>100MB): umya-spreadsheet loads entire DOM into memory — use `count_sheet_rows` first to check size
- URL cache has no TTL — delete `$TMPDIR/sheets_mcp/` manually to force re-download
- No chart/image preservation testing (umya-spreadsheet handles them but verify)

## License

MIT