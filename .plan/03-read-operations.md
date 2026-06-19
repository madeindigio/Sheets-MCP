# Phase 2: Read Operations

## Goal
Implement 3 read tools: `read_structure`, `count_sheet_rows`, `get_sheet_range`.

## Implementation Plan

### `src/reader.rs`

---

### Tool 1: `read_structure(source: &FileSource, first_n_rows: u32) -> Result<WorkbookStructure>`

The most important read tool. Returns the "layout" of the workbook so the LLM understands what it's working with. Includes the first N rows with **values AND formatting**.

**Input path** is resolved from `FileSource` (local path or cached URL download).

**Uses umya-spreadsheet** because calamine doesn't expose cell formatting (colors, fonts).

```rust
pub struct WorkbookStructure {
    pub file_path: String,
    pub sheets: Vec<SheetStructure>,
}

pub struct SheetStructure {
    pub name: String,
    pub total_rows: usize,
    pub total_columns: usize,
    /// The first N rows with values AND formatting (for column titles + example data)
    pub preview_rows: Vec<Vec<CellWithFormat>>,
}
```

Implementation:
1. `let path = source.resolve().await?` — get local path
2. `umya_spreadsheet::reader::xlsx::read(path)` — open workbook
3. For each sheet:
   - Get dimensions via `sheet.get_highest_column_and_row()`
   - Collect first N rows: iterate rows 0..first_n_rows, for each col get `sheet.get_cell((col, row))`
   - Extract value via `.get_value()` and formatting via style API (see format extraction below)

### Extracting Cell Format (umya-spreadsheet style API)

```rust
fn extract_format(cell: &umya_spreadsheet::structs::Cell) -> CellFormat {
    let style = cell.get_style();
    let fill = style.get_fill();
    let font = style.get_font();
    let alignment = style.get_alignment();
    let numfmt = style.get_number_format();

    CellFormat {
        background_color: argb_to_hex(fill.get_fill_background_color()),
        font_color: argb_to_hex(font.get_color()),
        bold: font.get_bold(),
        italic: font.get_italic(),
        font_size: font.get_size().map(|s| s.into()),
        number_format: numfmt.get_format_code().map(|s| s.to_string()),
        horizontal_alignment: alignment.get_horizontal().map(|a| format!("{:?}", a)),
        has_border: style_has_any_border(style),
    }
}
```

Color conversion: umya-spreadsheet uses ARGB `u32` (e.g., `0xFFC0C0C0`). Convert to hex string `"#C0C0C0"`.

---
source: &FileSource, sheet_name: &str) -> Result<RowCount>`

Fast row count. Uses **calamine** since no formatting needed.

```rust
pub struct RowCount {
    pub sheet_name: String,
    pub total_rows: usize,
    pub total_columns: usize,
}
```

Implementation:
- `let path = source.resolve().await?`
- `calamine::open_workbook_auto(
- `calamine::open_workbook_auto(file_path)`
- Get sheet by name, call `.worksheet_range(sheet_name)`
- Return `range.end()` → (row, col) dimensions

---

### Tool 3: `get_sheet_range(source: &FileSource, sheet_name, range: CellRange, include_format: bool) -> Result<RangeData>`

Read cell values from a specific range. Resolves source to local path, then dispatches to two code paths based on `include_format`:

- **`include_format = false`** (default): use **calamine** (fast, ~1M cells/sec)
- **`include_format = true`**: use **umya-spreadsheet** (slower, full DOM load)

```rust
pub struct RangeData {
    pub sheet_name: String,
    pub range: String,            // e.g. "A1:D100"
    pub rows: Vec<Vec<RangeCell>>,
}

pub struct RangeCell {
    pub value: CellValue,
    /// Only present when include_format = true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<CellFormat>,
}
```

### CSV Support — Removed
Per your 5-tool list, CSV is out of scope. The `csv` crate dependency is removed.

---

## Dependency Decision

| Tool | Crate | Reason |
|------|-------|--------|
| `read_structure` | umya-spreadsheet | Needs formatting |
| `count_sheet_rows` | calamine | Pure speed |
| `get_sheet_range` (no format) | calamine | Pure speed |
| `get_sheet_range` (with format) | umya-spreadsheet | Formatting needed |

## Acceptance Criteria
- [ ] `read_structure` returns sheet names, dimensions, and first N rows with values + colors + fonts + borders
- [ ] `read_structure` default `first_n_rows` is ~10 (enough for headers + sample data)
- [ ] `count_sheet_rows` returns accurate row/column counts
- [ ] `get_sheet_range` returns correct cell grid for a given range
- [ ] `get_sheet_range` with `include_format=true` returns colors, fonts, borders per cell
- [ ] All functions handle file-not-found and sheet-not-found gracefully