# Phase 3: Write Operations

## Goal
Implement 2 write tools using `umya-spreadsheet`: `update_cell` (single cell) and `update_cells` (array paste).

## Implementation Plan

### `src/writer.rs`

---

### Tool 4: `update_cell(source: &FileSource, output_path: Option<&str>, sheet_name, update: CellUpdate) -> Result<UpdateResult>`

Update a single cell's value.

**Input**: Resolved from `FileSource` (local path or cached URL download).
**Output**: If `output_path` is provided, writes there. If not and source is a `file_path`, writes back to same path. If source is a URL and no `output_path` → error.

```rust
pub struct CellUpdate {
    pub coordinate: String,   // e.g. "B5"
    pub value: String,        // new value as string ("42", "hello", "true")
    pub value_type: CellValueType,
}

pub enum CellValueType {
    String,
    Number,
    Bool,
}

pub struct UpdateResult {
    pub updated_count: usize,  // always 1
    pub sheet_name: String,
    pub coordinate: String,
    pub output_path: String,  // where the file was saved
}
```

Implementation:
- `let input_path = source.resolve().await?` — get local file
- `let output = source.write_target(output_path)?` — determine where to save
- `umya_spreadsheet::reader::xlsx::read(&input_path)` — open workbook
- `workbook.get_sheet_by_name_mut(&sheet_name)` — get mutable sheet
- `sheet.get_cell_mut(&coordinate)` — get mutable cell
- Set value based on type:
  - `String` → `cell.set_value(value)`
  - `Number` → parse as f64, `cell.set_value_number(parsed)`
  - `Bool` → parse "true"/"false", `cell.set_value_bool(parsed)`
- `umya_spreadsheet::writer::xlsx::write(&workbook, &output)` — save back

**Guarantees**:
- ✅ Only touches the target cell's value XML element
- ✅ Does NOT touch any cell's formula (`<f>` element)
- ✅ Does NOT touch any cell's style reference (`s=""` attribute)
- ✅ All other cells (including ones with `=A1+B1` formulas) remain untouched

---source: &FileSource, output_path: Option<&str>, sheet_name, destination: &str, data: Vec<Vec<String>>) -> Result<UpdateResult>`

Paste a 2D array of values starting at a destination cell.

```rust
pub struct UpdateCellsArgs {
    #[serde(flatten)]
    pub source: FileSource,
    pub sheet_name: String,
    pub destination: String,       // top-left cell e.g. "B2"
    pub data: Vec<Vec<String>>,    // 2D array of values
    pub output_path: Option<String>,
}

pub struct UpdateResult {
    pub updated_count: usize,      // total cells written
    pub sheet_name: String,
    pub start: String,             // destination cell
    pub rows: usize,
    pub columns: usize,
    pub output_path: String,       // where saved            // destination cell
    pub rows: usize,
    pub columns: usize,
}
```

Implementation:
- Parse `destination` coordinate into (start_col_index, start_row)
- Open workbook with umya-spreadsheet
- Get mutable sheet
- Iterate `data` rows → columns, compute coordinate from start + offset:
  - `CellCoordinate::from_index(col_index + c, row_index + r)`
  - `sheet.get_cell_mut(&computed_coord).set_value(value_string)`
- Save back

**Example**: `update_cells("file.xlsx", "Sheet1", "B2", [["x","y"],["z","w"]])`
```
Before:           After:
   A  B  C          A  B  C
1  .  .  .      1  .  .  .
2  .  .  .      2  .  x  y
3  .  .  .      3  .  z  w
```

**Same guarantees as `update_cell`** — only values are touched. Formulas and formatting in all cells (including overwritten ones) are preserved.

**Note**: If the destination cell currently has a formula, setting its value will overwrite the value but the formula stays in the XML. On next open, Excel may recalculate. If you truly need to remove a formula, that's a separate concern (not in scope).

---

## What's NOT in scope
- Creating new workbooks (simplified out)
- Adding/deleting sheets (simplified out)
- CSV writing (simplified out)
- Setting formulas (simplified out — we only write values)

## Acceptance Criteria
- [ ] `update_cell` changes a single cell's value without corrupting formulas or formatting
- [ ] `update_cells` pastes a 2D array starting at destination, preserving all existing formulas/formats
- [ ] `update_cells` correctly computes cell coordinates from row/column offsets
- [ ] Both tools handle errors: file-not-found, sheet-not-found, invalid coordinate

### Cell coordinate format
umya-spreadsheet uses a specific format: `sheet.get_cell_mut("A1")` — standard Excel A1 notation works.

### Formula safety — The Core Guarantee
umya-spreadsheet stores each cell as separate value and formula properties.
- `cell.set_value("42")` → writes `<v>42</v>` in the cell XML, does **NOT** touch any `<f>` (formula) element
- Cells with formulas have `<f>=A1+B1</f>` in their XML — this is never altered by value-only writes
- Result: updating A1 from 1→2 means C1 (with `=A1+B1`) will display 4 when reopened in Excel

### Format preservation — The Other Core Guarantee
umya-spreadsheet stores cell style as a separate style object referenced by index.
- `cell.set_value("42")` → only touches the `<v>` element, does **NOT** touch the style index (`s=""` attribute)
- This means: background color, font, borders, number format — **all preserved** on value-only writes
- Even `umya_spreadsheet::writer::xlsx::write()` serializes styles faithfully

**Under the hood**: When umya-spreadsheet writes back, it serializes each cell independently. Value updates and style/formula updates are separate operations. Updating cell A1's value does not touch cell C1's XML at all, nor does it alter any cell's formatting.

### Performance for large files
umya-spreadsheet loads the entire workbook into memory. For very large files (>50MB), consider:
- Reading with calamine first to check size
- Warning or batching updates

## Acceptance Criteria
- [ ] `update_cell` changes a single cell's value without corrupting formulas or formatting
- [ ] `update_cells` pastes a 2D array starting at destination, preserving all existing formulas/formats
- [ ] `update_cells` correctly computes cell coordinates from row/column offsets
- [ ] Both tools handle errors: file-not-found, sheet-not-found, invalid coordinate