# Phase 1: Domain Models & Types

## Goal
Define all shared types: cell coordinate parsing, workbook/sheet references, tool request/response structs, and the unified error type.

## Types to Define

### `src/types.rs`

#### CellCoordinate
Parse Excel-style coordinates like `A1`, `Z100`, `AA42` into (column_str, row_number).

```rust
pub struct CellCoordinate {
    pub column: String,   // e.g. "A", "AA"
    pub column_index: u32, // zero-based: A=0, B=1, ...
    pub row: u32,          // one-based: 1, 2, ...
}
```

Implement `FromStr` for `CellCoordinate` and `Display`.

#### CellRange
Parse ranges like `A1:B10`, `Sheet1!A1:Z100`.

```rust
pub struct CellRange {
    pub sheet: Option<String>,
    pub start: CellCoordinate,
    pub end: CellCoordinate,
}
```

#### CellValue
A unified cell value enum covering all Excel cell types:

```rust
pub enum CellValue {
    Empty,
    Bool(bool),
    Number(f64),
    String(String),
    Error(String),
}
```

#### CellFormat
Captures visual styling so an LLM can identify cells by appearance (e.g., "update the gray cells").

```rust
#[derive(Debug, Clone, Serialize)]
pub struct CellFormat {
    /// Background / fill color as hex (e.g. "#C0C0C0")
    pub background_color: Option<String>,
    /// Font color as hex
    pub font_color: Option<String>,
    /// Font weight
    pub bold: bool,
    /// Font style
    pub italic: bool,
    /// Font size in points
    pub font_size: Option<f64>,
    /// Number format string (e.g. "0.00%", "yyyy-mm-dd")
    pub number_format: Option<String>,
    /// Horizontal alignment
    pub horizontal_alignment: Option<String>,
    /// Border style summary
    pub has_border: bool,
}
```

#### CellWithFormat
Used in `read_structure` and `get_sheet_range` (with format):

```rust
pub struct CellWithFormat {
    pub coordinate: String,
    pub value: CellValue,
    pub format: CellFormat,
}
```

#### Tool Input/Output Structs

| # | Tool | Input Struct | Output Struct |
|---|------|-------------|---------------|
| 1 | `read_structure` | `{ file_path: String, first_n_rows: u32 }` | `WorkbookStructure { sheets: Vec<SheetStructure> }` |
| 2 | `count_sheet_rows` | `{ file_path: String, sheet_name: String }` | `RowCount { sheet_name, total_rows, total_columns }` |
| 3 | `get_sheet_range` | `{ file_path, sheet_name, range, include_format }` | `RangeData { sheet_name, range, rows: Vec<Vec<RangeCell>> }` |
| 4 | `update_cell` | `{ file_path \| url, sheet_name, coordinate, value, value_type, output_path? }` | `UpdateResult { updated_count, sheet_name, coordinate, output_path }` |
| 5 | `update_cells` | `{ file_path \| url, sheet_name, destination, data: Vec<Vec<String>>, output_path? }` | `UpdateResult { updated_count, start, rows, columns, output_path }` |

### `src/error.rs`

```rust
#[derive(Debug, thiserror::Error)]
pub enum SheetsError {
    #[error("File not found: {0}")]
    FileNotFound(String),
    #[error("Sheet not found: {0}")]
    SheetNotFound(String),
    #[error("Invalid cell coordinate: {0}")]
    InvalidCoordinate(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("Spreadsheet error: {0}")]
    Spreadsheet(String),
    #[error("Output path required when source is a URL")]
    MissingOutputPath,
}
```

## Acceptance Criteria
- [ ] `CellCoordinate` parses from `"A1"`, `"Z99"`, `"AA100"` correctly
- [ ] `CellCoordinate` displays back as `"A1"` format
- [ ] `CellRange` parses with and without sheet prefix
- [ ] All tool structs derive `Serialize`/`Deserialize`
- [ ] Error type converts cleanly to `rmcp` tool error responses