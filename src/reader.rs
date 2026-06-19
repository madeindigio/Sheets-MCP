//! Read operations for Excel workbooks.
//!
//! This module provides three read functions, each using the optimal backend:
//!
//! | Function | Backend | Speed | Formatting |
//! |----------|---------|-------|------------|
//! | [`read_structure`] | umya-spreadsheet | ~5ms | ✅ |
//! | [`count_sheet_rows`] | calamine | <1ms | ❌ |
//! | [`get_sheet_range`] (no format) | calamine | ~1M cells/sec | ❌ |
//! | [`get_sheet_range`] (with format) | umya-spreadsheet | ~10K cells/sec | ✅ |

use std::path::Path;

use calamine::Reader as _;

use crate::error::SheetsError;
use crate::file_source::FileSource;
use crate::types::{
    argb_to_hex, CellFormat, CellValue, CellWithFormat,
    RangeCell, RangeData, RowCount, SheetStructure,
    WorkbookStructure,
};

// ──────────────────────────────────────────────
// read_structure
// ──────────────────────────────────────────────

/// Returns the workbook structure with the first N rows including values
/// and formatting (colors, fonts, borders).
///
/// Uses umya-spreadsheet to load the full DOM. Returns one [`SheetStructure`]
/// per sheet in the workbook, each containing dimensions and preview rows.
///
/// # Arguments
/// * `source` - Local file path or remote URL
/// * `first_n_rows` - Number of preview rows to include per sheet (default: 10)
///
/// # Errors
/// Returns [`SheetsError::FileNotFound`] if the file doesn't exist,
/// or [`SheetsError::Spreadsheet`] if the file can't be parsed.
pub async fn read_structure(
    source: &FileSource,
    first_n_rows: u32,
) -> Result<WorkbookStructure, SheetsError> {
    let path = source.resolve().await?;
    let workbook = read_umya(&path)?;

    let mut sheets = Vec::new();

    for ws in workbook.get_sheet_collection() {
        let (max_col, max_row) = ws.get_highest_column_and_row();
        let mut preview = Vec::new();

        for row in 1..=first_n_rows.min(max_row) {
            let mut row_cells = Vec::new();
            for col in 1..=max_col {
                let coord = format!(
                    "{}{}",
                    crate::types::col_index_to_letter(col - 1),
                    row
                );
                let cell_with_format = match ws.get_cell((col, row)) {
                    Some(cell) => {
                        let value = cell_value_from_umya(cell);
                        let format = extract_format(cell);
                        CellWithFormat {
                            coordinate: coord,
                            value,
                            format,
                        }
                    }
                    None => CellWithFormat {
                        coordinate: coord,
                        value: CellValue::Empty,
                        format: CellFormat::default(),
                    },
                };
                row_cells.push(cell_with_format);
            }
            preview.push(row_cells);
        }

        sheets.push(SheetStructure {
            name: ws.get_name().to_string(),
            total_columns: max_col,
            total_rows: max_row,
            preview,
        });
    }

    Ok(WorkbookStructure { sheets })
}

// ──────────────────────────────────────────────
// count_sheet_rows
// ──────────────────────────────────────────────

/// Returns the total row and column count for a specific sheet.
///
/// Uses calamine for fast metadata reading (no formatting, no DOM load).
/// This is the fastest way to determine sheet dimensions.
///
/// # Arguments
/// * `source` - Local file path or remote URL
/// * `sheet_name` - Name of the sheet to query
///
/// # Errors
/// Returns [`SheetsError::SheetNotFound`] if the sheet name doesn't match
/// any sheet in the workbook (available names are listed in the error).
pub async fn count_sheet_rows(
    source: &FileSource,
    sheet_name: &str,
) -> Result<RowCount, SheetsError> {
    let path = source.resolve().await?;
    let mut workbook = open_calamine(&path)?;

    let range = workbook.worksheet_range(sheet_name).map_err(|_e| {
        let names: Vec<String> = workbook
            .sheet_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        SheetsError::sheet_not_found(sheet_name, &names)
    })?;
    let (total_columns, total_rows) = range
        .end()
        .map(|(r, c)| (c + 1, r + 1)) // calamine is 0-based, convert to 1-based
        .unwrap_or((0, 0));

    Ok(RowCount {
        sheet_name: sheet_name.to_string(),
        total_rows,
        total_columns,
    })
}

// ──────────────────────────────────────────────
// get_sheet_range
// ──────────────────────────────────────────────

/// Returns cell values (and optionally formatting) from a range.
///
/// Dispatches to calamine (values only) or umya-spreadsheet (values + format)
/// based on the `include_format` flag.
///
/// # Arguments
/// * `source` - Local file path or remote URL
/// * `sheet_name` - Name of the sheet
/// * `range_str` - Range string like `"A1:D100"`
/// * `include_format` - If `true`, also extract colors/fonts/borders (slower)
///
/// # Performance
/// - `include_format = false`: ~1M cells/sec (calamine)
/// - `include_format = true`: ~10K cells/sec (umya-spreadsheet DOM)
pub async fn get_sheet_range(
    source: &FileSource,
    sheet_name: &str,
    range_str: &str,
    include_format: bool,
) -> Result<RangeData, SheetsError> {
    let path = source.resolve().await?;

    if include_format {
        get_range_with_format(&path, sheet_name, range_str).await
    } else {
        get_range_values_only(&path, sheet_name, range_str).await
    }
}

/// Fast path: calamine, values only.
async fn get_range_values_only(
    path: &Path,
    sheet_name: &str,
    range_str: &str,
) -> Result<RangeData, SheetsError> {
    let mut workbook = open_calamine(path)?;
    let range = workbook.worksheet_range(sheet_name).map_err(|_e| {
        // Enrich calamine error with available sheet names.
        let names: Vec<String> = workbook
            .sheet_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        SheetsError::sheet_not_found(sheet_name, &names)
    })?;

    let parsed: crate::types::CellRange = range_str.parse()?;
    let start_row = parsed.start.row; // 1-based
    let start_col = parsed.start.column_index; // 0-based
    let end_row = parsed.end.row; // 1-based
    let end_col = parsed.end.column_index; // 0-based

    let mut rows = Vec::new();

    for row in start_row..=end_row {
        let mut row_cells = Vec::new();
        for col in start_col..=end_col {
            let value = range
                .get_value((row - 1, col)) // calamine is 0-based
                .map(cell_value_from_calamine)
                .unwrap_or(CellValue::Empty);
            row_cells.push(RangeCell {
                value,
                format: None,
            });
        }
        rows.push(row_cells);
    }

    Ok(RangeData {
        sheet_name: sheet_name.to_string(),
        range: range_str.to_string(),
        rows,
    })
}

/// Slow path: umya-spreadsheet, values + formatting.
async fn get_range_with_format(
    path: &Path,
    sheet_name: &str,
    range_str: &str,
) -> Result<RangeData, SheetsError> {
    let workbook = read_umya(path)?;
    let ws = workbook
        .get_sheet_by_name(sheet_name)
        .ok_or_else(|| {
            let names: Vec<String> = workbook
                .get_sheet_collection()
                .iter()
                .map(|s| s.get_name().to_string())
                .collect();
            SheetsError::sheet_not_found(sheet_name, &names)
        })?;

    let parsed: crate::types::CellRange = range_str.parse()?;
    let start_row = parsed.start.row; // 1-based
    let start_col = parsed.start.column_index; // 0-based
    let end_row = parsed.end.row; // 1-based
    let end_col = parsed.end.column_index; // 0-based

    let mut rows = Vec::new();

    for row in start_row..=end_row {
        let mut row_cells = Vec::new();
        for col in start_col..=end_col {
            let umya_col = col + 1; // umya is 1-based
            let cell_with_format = match ws.get_cell((umya_col, row)) {
                Some(cell) => {
                    let value = cell_value_from_umya(cell);
                    let format = extract_format(cell);
                    RangeCell {
                        value,
                        format: Some(format),
                    }
                }
                None => RangeCell {
                    value: CellValue::Empty,
                    format: Some(CellFormat::default()),
                },
            };
            row_cells.push(cell_with_format);
        }
        rows.push(row_cells);
    }

    Ok(RangeData {
        sheet_name: sheet_name.to_string(),
        range: range_str.to_string(),
        rows,
    })
}

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

/// Read an `.xlsx` file using umya-spreadsheet.
///
/// Returns the full spreadsheet DOM. Use this when you need formatting data.
fn read_umya(
    path: &Path,
) -> Result<umya_spreadsheet::Spreadsheet, SheetsError> {
    umya_spreadsheet::reader::xlsx::read(path)
        .map_err(|e| SheetsError::Spreadsheet(e.to_string()))
}

/// Open a workbook using calamine.
///
/// Supports `.xlsx` and `.xls` formats. Use this for fast values-only reads.
fn open_calamine(
    path: &Path,
) -> Result<
    calamine::Sheets<std::io::BufReader<std::fs::File>>,
    SheetsError,
> {
    calamine::open_workbook_auto(path)
        .map_err(|e| SheetsError::Spreadsheet(e.to_string()))
}

/// Convert a calamine [`Data`](calamine::Data) variant to our [`CellValue`].
///
/// Maps: `Int`/`Float` → `Number`, `String` → `String`, `Bool` → `Bool`,
/// `Error` → `Error`, `DateTime` → `String`, `Empty` → `Empty`.
fn cell_value_from_calamine(data: &calamine::Data) -> CellValue {
    use calamine::Data;
    match data {
        Data::Int(i) => CellValue::Number(*i as f64),
        Data::Float(f) => CellValue::Number(*f),
        Data::String(s) => CellValue::String(s.clone()),
        Data::Bool(b) => CellValue::Bool(*b),
        Data::Error(e) => CellValue::Error(format!("{:?}", e)),
        Data::DateTime(dt) => {
            CellValue::String(dt.to_string())
        }
        Data::DateTimeIso(s) => CellValue::String(s.clone()),
        Data::DurationIso(s) => CellValue::String(s.clone()),
        Data::Empty => CellValue::Empty,
    }
}

/// Convert an umya-spreadsheet [`Cell`](umya_spreadsheet::structs::Cell) to our [`CellValue`].
///
/// Tries to parse the raw string as `f64` (→ `Number`) or `bool` (→ `Bool`),
/// otherwise returns `String`. Empty cells return `Empty`.
fn cell_value_from_umya(
    cell: &umya_spreadsheet::structs::Cell,
) -> CellValue {
    let raw = cell.get_value().to_string();
    if raw.is_empty() {
        return CellValue::Empty;
    }
    // Try parsing as number.
    if let Ok(n) = raw.parse::<f64>() {
        return CellValue::Number(n);
    }
    // Try parsing as bool.
    if raw.eq_ignore_ascii_case("true") {
        return CellValue::Bool(true);
    }
    if raw.eq_ignore_ascii_case("false") {
        return CellValue::Bool(false);
    }
    CellValue::String(raw)
}

/// Extract visual formatting from an umya-spreadsheet cell.
///
/// Reads background color, font properties, number format, alignment,
/// and border presence from the cell's style.
fn extract_format(
    cell: &umya_spreadsheet::structs::Cell,
) -> CellFormat {
    let style = cell.get_style();

    // Background color.
    let background_color = style
        .get_fill()
        .and_then(|f| f.get_pattern_fill())
        .and_then(|pf| pf.get_background_color())
        .and_then(|c| argb_to_hex(parse_argb_u32(c.get_argb())));

    // Font properties.
    let (font_color, bold, italic, font_size) =
        match style.get_font() {
            Some(font) => {
                let color = font
                    .get_color()
                    .get_argb()
                    .to_string();
                let fc = argb_to_hex(parse_argb_u32(&color));
                let b = *font.get_bold();
                let i = *font.get_italic();
                let s = *font.get_font_size().get_val();
                (fc, b, i, Some(s))
            }
            None => (None, false, false, None),
        };

    // Number format.
    let number_format = style
        .get_number_format()
        .and_then(|nf| {
            let code = nf.get_format_code().to_string();
            if code.is_empty() || code == "General" {
                None
            } else {
                Some(code)
            }
        });

    // Horizontal alignment.
    let horizontal_alignment = style
        .get_alignment()
        .and_then(|a| {
            let h = a.get_horizontal();
            // Only return if not the default (start/general).
            let s = format!("{:?}", h);
            if s == "General" || s == "Start" {
                None
            } else {
                Some(s)
            }
        });

    // Border detection.
    let has_border = style
        .get_borders()
        .map(|borders| {
            !borders.get_left().get_border_style().is_empty()
                || !borders.get_right().get_border_style().is_empty()
                || !borders.get_top().get_border_style().is_empty()
                || !borders.get_bottom().get_border_style().is_empty()
        })
        .unwrap_or(false);

    CellFormat {
        background_color,
        font_color,
        bold,
        italic,
        font_size,
        number_format,
        horizontal_alignment,
        has_border,
    }
}

/// Parse an ARGB hex string like "FFC0C0C0" into a u32.
fn parse_argb_u32(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or(0)
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_source::FileSource;
    use umya_spreadsheet::new_file;

    /// Create a test .xlsx file with sample data and return its path.
    fn create_test_file(name: &str) -> std::path::PathBuf {
        let mut book = new_file();
        let ws = book.get_sheet_mut(&0).unwrap();

        // Headers (row 1).
        ws.get_cell_mut("A1").set_value("Name");
        ws.get_cell_mut("B1").set_value("Score");
        ws.get_cell_mut("C1").set_value("Grade");

        // Data (rows 2-4).
        ws.get_cell_mut("A2").set_value("Alice");
        ws.get_cell_mut("B2").set_value("95");
        ws.get_cell_mut("C2").set_value("=IF(B2>=90,\"A\",\"B\")");

        ws.get_cell_mut("A3").set_value("Bob");
        ws.get_cell_mut("B3").set_value("78");
        ws.get_cell_mut("C3").set_value("=IF(B3>=90,\"A\",\"B\")");

        ws.get_cell_mut("A4").set_value("Charlie");
        ws.get_cell_mut("B4").set_value("88");
        ws.get_cell_mut("C4").set_value("=IF(B4>=90,\"A\",\"B\")");

        let dir = std::env::temp_dir().join("sheets_mcp_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.xlsx"));
        umya_spreadsheet::writer::xlsx::write(&book, &path).unwrap();
        path
    }

    #[tokio::test]
    async fn read_structure_returns_all_sheets() {
        let path = create_test_file("structure_test");
        let source = FileSource::Path {
            file_path: path.to_string_lossy().to_string(),
        };
        let result = read_structure(&source, 10).await.unwrap();
        assert_eq!(result.sheets.len(), 1);
        let sheet = &result.sheets[0];
        assert_eq!(sheet.name, "Sheet1");
        assert_eq!(sheet.total_columns, 3);
        assert_eq!(sheet.total_rows, 4);
    }

    #[tokio::test]
    async fn read_structure_preview_rows() {
        let path = create_test_file("preview_test");
        let source = FileSource::Path {
            file_path: path.to_string_lossy().to_string(),
        };
        let result = read_structure(&source, 2).await.unwrap();
        let sheet = &result.sheets[0];
        assert_eq!(sheet.preview.len(), 2);
        // First row should be headers.
        assert_eq!(
            sheet.preview[0][0].value,
            CellValue::String("Name".to_string())
        );
        assert_eq!(
            sheet.preview[0][1].value,
            CellValue::String("Score".to_string())
        );
        // Second row should be first data row.
        assert_eq!(
            sheet.preview[1][0].value,
            CellValue::String("Alice".to_string())
        );
    }

    #[tokio::test]
    async fn read_structure_fewer_rows_than_available() {
        let path = create_test_file("fewer_rows");
        let source = FileSource::Path {
            file_path: path.to_string_lossy().to_string(),
        };
        // Only ask for 2 rows but 4 exist.
        let result = read_structure(&source, 2).await.unwrap();
        let sheet = &result.sheets[0];
        assert_eq!(sheet.preview.len(), 2);
    }

    #[tokio::test]
    async fn count_sheet_rows_correct() {
        let path = create_test_file("count_test");
        let source = FileSource::Path {
            file_path: path.to_string_lossy().to_string(),
        };
        let result = count_sheet_rows(&source, "Sheet1").await.unwrap();
        assert_eq!(result.total_rows, 4);
        assert_eq!(result.total_columns, 3);
    }

    #[tokio::test]
    async fn count_sheet_rows_not_found() {
        let path = create_test_file("count_notfound");
        let source = FileSource::Path {
            file_path: path.to_string_lossy().to_string(),
        };
        let result = count_sheet_rows(&source, "NonExistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_range_values_only() {
        let path = create_test_file("range_values");
        let source = FileSource::Path {
            file_path: path.to_string_lossy().to_string(),
        };
        let result =
            get_sheet_range(&source, "Sheet1", "A1:B2", false)
                .await
                .unwrap();
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0].len(), 2);
        assert_eq!(
            result.rows[0][0].value,
            CellValue::String("Name".to_string())
        );
        assert_eq!(
            result.rows[0][1].value,
            CellValue::String("Score".to_string())
        );
        assert_eq!(
            result.rows[1][0].value,
            CellValue::String("Alice".to_string())
        );
        // Format should be None (fast path).
        assert!(result.rows[0][0].format.is_none());
    }

    #[tokio::test]
    async fn get_range_with_format() {
        let path = create_test_file("range_format");
        let source = FileSource::Path {
            file_path: path.to_string_lossy().to_string(),
        };
        let result =
            get_sheet_range(&source, "Sheet1", "A1:B2", true)
                .await
                .unwrap();
        assert_eq!(result.rows.len(), 2);
        // Format should be Some (format path).
        assert!(result.rows[0][0].format.is_some());
        assert_eq!(
            result.rows[0][0].value,
            CellValue::String("Name".to_string())
        );
    }

    #[tokio::test]
    async fn get_range_out_of_bounds_returns_empty() {
        let path = create_test_file("range_empty");
        let source = FileSource::Path {
            file_path: path.to_string_lossy().to_string(),
        };
        // Request a range beyond the data.
        let result =
            get_sheet_range(&source, "Sheet1", "E10:F11", false)
                .await
                .unwrap();
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0].len(), 2);
        // Cells outside data should be empty.
        assert_eq!(result.rows[0][0].value, CellValue::Empty);
    }

    #[tokio::test]
    async fn read_structure_file_not_found() {
        let source = FileSource::Path {
            file_path: "/tmp/nonexistent_file_12345.xlsx".to_string(),
        };
        let result = read_structure(&source, 10).await;
        assert!(result.is_err());
    }

    /// Create a test .xlsx with formatted cells and return its path.
    fn create_formatted_test_file(name: &str) -> std::path::PathBuf {
        let mut book = new_file();
        let ws = book.get_sheet_mut(&0).unwrap();

        // Set a header with bold font.
        let header = ws.get_cell_mut("A1");
        header.set_value("Header");
        header.get_style_mut().get_font_mut().set_bold(true);

        // Set a cell with background color.
        let colored = ws.get_cell_mut("B1");
        colored.set_value("Colored");
        let fill = colored
            .get_style_mut()
            .get_fill_mut();
        fill.get_pattern_fill_mut()
            .get_background_color_mut()
            .set_argb("FFFF0000");

        // Set a cell with a border.
        let bordered = ws.get_cell_mut("C1");
        bordered.set_value("Bordered");
        let borders = bordered
            .get_style_mut()
            .get_borders_mut();
        borders
            .get_left_border_mut()
            .set_border_style("thin");

        // Data rows.
        ws.get_cell_mut("A2").set_value("Alice");
        ws.get_cell_mut("B2").set_value("42");
        ws.get_cell_mut("C2").set_value("Yes");

        let dir = std::env::temp_dir().join("sheets_mcp_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.xlsx"));
        umya_spreadsheet::writer::xlsx::write(&book, &path).unwrap();
        path
    }

    #[tokio::test]
    async fn read_structure_returns_formatting() {
        let path = create_formatted_test_file("format_test");
        let source = FileSource::Path {
            file_path: path.to_string_lossy().to_string(),
        };
        let result = read_structure(&source, 2).await.unwrap();
        let sheet = &result.sheets[0];
        assert_eq!(sheet.preview.len(), 2);

        // A1 should be bold.
        assert!(sheet.preview[0][0].format.bold);
        // B1 should have a background color.
        assert!(sheet.preview[0][1].format.background_color.is_some());
        // C1 should have a border.
        assert!(sheet.preview[0][2].format.has_border);
    }

    #[tokio::test]
    async fn get_range_with_format_returns_styles() {
        let path = create_formatted_test_file("format_range_test");
        let source = FileSource::Path {
            file_path: path.to_string_lossy().to_string(),
        };
        let result =
            get_sheet_range(&source, "Sheet1", "A1:C1", true).await.unwrap();
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 3);
        // Each cell should have format.
        assert!(result.rows[0][0].format.is_some());
        assert!(result.rows[0][1].format.is_some());
        assert!(result.rows[0][2].format.is_some());
        // Bold check.
        assert!(result.rows[0][0].format.as_ref().unwrap().bold);
        // Background color check.
        assert!(
            result.rows[0][1]
                .format.as_ref().unwrap()
                .background_color.is_some()
        );
    }

    #[tokio::test]
    async fn sheet_not_found_lists_available() {
        let path = create_test_file("sheet_not_found_test");
        let source = FileSource::Path {
            file_path: path.to_string_lossy().to_string(),
        };
        let result = count_sheet_rows(&source, "WrongSheet").await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Sheet1"), "Error should list available sheets: {err_msg}");
    }
}
