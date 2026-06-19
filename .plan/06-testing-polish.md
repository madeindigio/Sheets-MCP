# Phase 5: Testing & Polish

## Goal
Add tests, improve error messages, add documentation, and prepare for release.

## Steps

### 1. Unit Tests

#### `src/types.rs` tests
- `CellCoordinate::from_str("A1")` → `{ col: "A", col_index: 0, row: 1 }`
- `CellCoordinate::from_str("Z100")` → `{ col: "Z", col_index: 25, row: 100 }`
- `CellCoordinate::from_str("AA1")` → `{ col: "AA", col_index: 26, row: 1 }`
- `CellCoordinate::from_str("")` → Error
- `CellCoordinate::from_str("1A")` → Error
- `CellRange::from_str("A1:B10")` → valid range
- `CellCoordinate::from_index(col, row)` → correct string coordinate

#### `src/reader.rs` tests
- Create test `.xlsx` fixtures with known data
- Test `read_structure` returns correct sheet names, dimensions, and preview rows
- Test `read_structure` returns formatting (colors, fonts, borders)
- Test `count_sheet_rows` returns correct counts
- Test `get_sheet_range` returns correct values for a range
- Test `get_sheet_range` with `include_format=true` returns formatting

#### `src/writer.rs` tests
- Test `update_cell` changes value without corrupting formulas or formatting
- Test `update_cells` pastes 2D array correctly starting at destination
- Test `update_cells` coordinate computation from offsets

### 2. Test Fixtures

Create a `tests/fixtures/` directory:
- `simple.xlsx` — 3x3 grid with strings and numbers
- `formulas.xlsx` — cells with formulas like `=SUM(A1:A3)`
- `formatted.xlsx` — cells with different background colors, fonts, borders
- `large.xlsx` — moderate-sized workbook for perf testing
- `legacy.xls` — if supporting .xls

### 3. Formula Safety Test
- Create `.xlsx` with formulas (e.g., `C1 = A1 + B1`)
- Update A1 via `update_cells`, save, re-read with calamine
- Verify: C1 still has its formula intact
- Verify: C1's computed value reflects the new A1 value

### 4. Format Safety Test
- Create `.xlsx` with styled cells (red background, bold text, borders)
- Update those cells' values via `update_cells`, save, re-open with umya
- Verify: background color, font, and borders are untouched
- Verify: `read_cells_with_format` returns the same format data before and after update

### 4. Integration Tests

`tests/integration.rs`:
- End-to-end test: `update_cell` → `get_sheet_range` → verify value changed
- Array paste test: `update_cells` with 2D array → `get_sheet_range` → verify grid
- Format preservation: update cell → `get_sheet_range` with `include_format=true` → verify format unchanged

### 5. Error Message Polish

- All errors should include the file path that failed
- Cell coordinate errors should show the invalid input
- Sheet-not-found errors should list available sheet names

### 5. Performance Notes

- calamine reading: ~1.1M cells/sec — should be instant for typical files
- umya-spreadsheet write: full DOM load + save — ~5-10ms for small files, proportional to file size
- Memory footprint: ~8-12MB idle, scales with workbook size when processing

### 6. Release Checklist

- [ ] `cargo build --release` produces a working binary
- [ ] Binary size is reasonable (<20MB)
- [ ] All tests pass: `cargo test`
- [ ] `cargo clippy` passes without warnings
- [ ] Ready for MCP client configuration

## MCP Client Configuration

```json
{
  "mcpServers": {
    "sheets": {
      "command": "/path/to/sheets_mcp/target/release/sheets_mcp"
    }
  }
}
```