//! Domain types for the sheets_mcp MCP server.
//!
//! Contains cell coordinates, ranges, values, formats, and all tool I/O structs.
//!
//! # Coordinate System
//!
//! - **Column letters**: `A`=0, `B`=1, ..., `Z`=25, `AA`=26, ...
//! - **Row numbers**: 1-based (Excel convention)
//! - **Cell coordinate**: e.g. `A1`, `AA42`
//! - **Cell range**: e.g. `A1:B10`, `Sheet1!A1:Z100`

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::error::SheetsError;

// ──────────────────────────────────────────────
// CellCoordinate
// ──────────────────────────────────────────────

/// A single cell coordinate, e.g. `A1`, `AA42`.
///
/// Parses the letter(s) into a zero-based column index and
/// the digits into a one-based row number.
///
/// # Examples
///
/// ```rust
/// use sheets_mcp::types::CellCoordinate;
///
/// let c: CellCoordinate = "A1".parse().unwrap();
/// assert_eq!(c.column, "A");
/// assert_eq!(c.column_index, 0);
/// assert_eq!(c.row, 1);
///
/// let c: CellCoordinate = "AA42".parse().unwrap();
/// assert_eq!(c.column_index, 26);
/// assert_eq!(c.row, 42);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellCoordinate {
    /// Column letters, e.g. `"A"`, `"AA"`.
    pub column: String,
    /// Zero-based column index: A=0, B=1, ... Z=25, AA=26, ...
    pub column_index: u32,
    /// One-based row number.
    pub row: u32,
}

impl CellCoordinate {
    /// Build directly from known parts.
    pub fn from_index(col_index: u32, row: u32) -> Self {
        let column = col_index_to_letter(col_index);
        CellCoordinate {
            column,
            column_index: col_index,
            row,
        }
    }
}

impl FromStr for CellCoordinate {
    type Err = SheetsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // Split into leading letters (column) and trailing digits (row).
        let split = s
            .char_indices()
            .find(|(_, c)| c.is_ascii_digit())
            .ok_or_else(|| {
                SheetsError::InvalidCoordinate(format!(
                    "'{s}' has no row number"
                ))
            })?;

        let (col_part, row_part) = s.split_at(split.0);

        if col_part.is_empty() {
            return Err(SheetsError::InvalidCoordinate(format!(
                "'{s}' has no column letters"
            )));
        }

        let col_index = letter_to_col_index(col_part)?;
        let row: u32 = row_part.parse().map_err(|_| {
            SheetsError::InvalidCoordinate(format!(
                "'{s}' has an invalid row number"
            ))
        })?;

        if row == 0 {
            return Err(SheetsError::InvalidCoordinate(format!(
                "'{s}' row must be >= 1"
            )));
        }

        Ok(CellCoordinate {
            column: col_part.to_uppercase(),
            column_index: col_index,
            row,
        })
    }
}

impl fmt::Display for CellCoordinate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.column, self.row)
    }
}

/// Convert zero-based column index to Excel-style letters.
/// 0→"A", 25→"Z", 26→"AA", etc.
pub fn col_index_to_letter(index: u32) -> String {
    let mut result = String::new();
    let mut n = index;
    loop {
        let remainder = n % 26;
        result.push((b'A' + remainder as u8) as char);
        n /= 26;
        if n == 0 {
            break;
        }
        n -= 1; // because 0 maps to "A" not ""
    }
    result.chars().rev().collect()
}

/// Returns true for legacy Excel 97-2003 workbooks (`.xls`).
pub(crate) fn is_legacy_xls(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("xls"))
        .unwrap_or(false)
}

/// Convert Excel-style column letters to zero-based index.
/// "A"→0, "Z"→25, "AA"→26, etc.
fn letter_to_col_index(s: &str) -> Result<u32, SheetsError> {
    let mut index: u32 = 0;
    for ch in s.chars() {
        let ch = ch.to_ascii_uppercase();
        if !ch.is_ascii_uppercase() {
            return Err(SheetsError::InvalidCoordinate(format!(
                "Invalid column character: '{ch}'"
            )));
        }
        index = index * 26 + (ch as u32 - 'A' as u32 + 1);
    }
    // Convert from 1-based to 0-based.
    Ok(index - 1)
}

// ──────────────────────────────────────────────
// CellRange
// ──────────────────────────────────────────────

/// A cell range, optionally prefixed with a sheet name.
///
/// Parses strings like `A1:B10` or `Sheet1!A1:Z100`.
///
/// # Examples
///
/// ```rust
/// use sheets_mcp::types::CellRange;
///
/// let r: CellRange = "A1:B10".parse().unwrap();
/// assert!(r.sheet.is_none());
/// assert_eq!(r.start.to_string(), "A1");
/// assert_eq!(r.end.to_string(), "B10");
///
/// let r: CellRange = "Sheet1!A1:Z100".parse().unwrap();
/// assert_eq!(r.sheet.as_deref(), Some("Sheet1"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellRange {
    /// Optional sheet name (everything before the `!`).
    pub sheet: Option<String>,
    /// Top-left coordinate.
    pub start: CellCoordinate,
    /// Bottom-right coordinate.
    pub end: CellCoordinate,
}

impl FromStr for CellRange {
    type Err = SheetsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // Split on '!' to separate sheet name.
        let (sheet, range_part) = if let Some(bang) = s.find('!') {
            (Some(s[..bang].to_string()), &s[bang + 1..])
        } else {
            (None, s)
        };

        let colon = range_part.find(':').ok_or_else(|| {
            SheetsError::InvalidCoordinate(format!(
                "Range '{s}' must contain ':'"
            ))
        })?;

        let start =
            CellCoordinate::from_str(&range_part[..colon])?;
        let end =
            CellCoordinate::from_str(&range_part[colon + 1..])?;

        Ok(CellRange {
            sheet,
            start,
            end,
        })
    }
}

impl fmt::Display for CellRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(sheet) = &self.sheet {
            write!(f, "{}!{}:{}", sheet, self.start, self.end)
        } else {
            write!(f, "{}:{}", self.start, self.end)
        }
    }
}

// ──────────────────────────────────────────────
// CellValue
// ──────────────────────────────────────────────

/// Unified representation of an Excel cell value.
///
/// Serialized as a tagged enum for JSON:
///
/// - `{ "type": "Empty" }`
/// - `{ "type": "Bool", "value": true }`
/// - `{ "type": "Number", "value": 42.0 }`
/// - `{ "type": "String", "value": "hello" }`
/// - `{ "type": "Error", "value": "#REF!" }`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum CellValue {
    /// Empty cell (no value).
    Empty,
    /// Boolean value.
    Bool(bool),
    /// Numeric value.
    Number(f64),
    /// String value.
    String(String),
    /// Excel error (e.g. `#REF!`, `#N/A`).
    Error(String),
}

// ──────────────────────────────────────────────
// CellFormat
// ──────────────────────────────────────────────

/// Visual styling of a cell.
///
/// Extracted from umya-spreadsheet cell styles. All fields are optional
/// or have sensible defaults. Colors are in `"#RRGGBB"` hex format.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CellFormat {
    /// Background / fill color as hex (e.g. `"#C0C0C0"`).
    pub background_color: Option<String>,
    /// Font color as hex.
    pub font_color: Option<String>,
    /// Whether the font is bold.
    pub bold: bool,
    /// Whether the font is italic.
    pub italic: bool,
    /// Font size in points.
    pub font_size: Option<f64>,
    /// Number format code (e.g. `"0.00%"`, `"yyyy-mm-dd"`, `"#,##0"`).
    pub number_format: Option<String>,
    /// Horizontal alignment (e.g. `"Center"`, `"Right"`).
    pub horizontal_alignment: Option<String>,
    /// Whether the cell has any border (left, right, top, or bottom).
    pub has_border: bool,
}

// ──────────────────────────────────────────────
// CellWithFormat
// ──────────────────────────────────────────────

/// A cell with its coordinate, value, and full formatting metadata.
///
/// Returned by `read_structure` for preview rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellWithFormat {
    /// Cell coordinate string, e.g. `"A1"`.
    pub coordinate: String,
    /// Cell value.
    pub value: CellValue,
    /// Cell visual formatting.
    pub format: CellFormat,
}

// ──────────────────────────────────────────────
// Tool I/O structs
// ──────────────────────────────────────────────

/// Sheet structure as returned by [`read_structure`](crate::reader::read_structure).
///
/// Contains the sheet name, dimensions, and preview rows with full formatting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SheetStructure {
    /// Sheet name (e.g. `"Sheet1"`).
    pub name: String,
    /// Total number of columns in the sheet.
    pub total_columns: u32,
    /// Total number of rows in the sheet.
    pub total_rows: u32,
    /// Preview rows with cell values and formatting.
    pub preview: Vec<Vec<CellWithFormat>>,
}

/// Top-level response for [`read_structure`](crate::reader::read_structure).
///
/// Contains one [`SheetStructure`] per sheet in the workbook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbookStructure {
    /// All sheets in the workbook.
    pub sheets: Vec<SheetStructure>,
}

/// Response for [`count_sheet_rows`](crate::reader::count_sheet_rows).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowCount {
    /// Name of the queried sheet.
    pub sheet_name: String,
    /// Total number of rows in the sheet.
    pub total_rows: u32,
    /// Total number of columns in the sheet.
    pub total_columns: u32,
}

/// A cell in range output: value + optional formatting.
///
/// Formatting is only present when `include_format = true`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeCell {
    /// Cell value.
    pub value: CellValue,
    /// Cell formatting (only present when `include_format = true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<CellFormat>,
}

/// Response for [`get_sheet_range`](crate::reader::get_sheet_range).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeData {
    /// Name of the sheet.
    pub sheet_name: String,
    /// The range string (e.g. `"A1:D100"`).
    pub range: String,
    /// 2D array of cells (row-major order).
    pub rows: Vec<Vec<RangeCell>>,
}

/// Response for [`update_cell`](crate::writer::update_cell) / [`update_cells`](crate::writer::update_cells).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    /// Number of cells updated.
    pub updated_count: u32,
    /// Name of the sheet that was modified.
    pub sheet_name: String,
    /// Path to the output file (where the result was written).
    pub output_path: String,
    /// The starting cell coordinate (for `update_cells` this is the destination).
    pub start: Option<String>,
    /// Number of rows affected (for `update_cells`).
    pub rows: Option<u32>,
    /// Number of columns affected (for `update_cells`).
    pub columns: Option<u32>,
}

/// Convert an umya-spreadsheet ARGB u32 to a hex string like `"#FF0000"`.
///
/// Strips the alpha byte and returns `None` if the input is zero (no color).
pub fn argb_to_hex(argb: u32) -> Option<String> {
    if argb == 0 {
        return None;
    }
    let r = (argb >> 16) & 0xFF;
    let g = (argb >> 8) & 0xFF;
    let b = argb & 0xFF;
    Some(format!("#{:02X}{:02X}{:02X}", r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CellCoordinate ──────────────────────

    #[test]
    fn parse_simple() {
        let c: CellCoordinate = "A1".parse().unwrap();
        assert_eq!(c.column, "A");
        assert_eq!(c.column_index, 0);
        assert_eq!(c.row, 1);
    }

    #[test]
    fn parse_z99() {
        let c: CellCoordinate = "Z99".parse().unwrap();
        assert_eq!(c.column, "Z");
        assert_eq!(c.column_index, 25);
        assert_eq!(c.row, 99);
    }

    #[test]
    fn parse_double_letter() {
        let c: CellCoordinate = "AA1".parse().unwrap();
        assert_eq!(c.column, "AA");
        assert_eq!(c.column_index, 26);
        assert_eq!(c.row, 1);
    }

    #[test]
    fn parse_double_letter_2() {
        let c: CellCoordinate = "AB10".parse().unwrap();
        assert_eq!(c.column, "AB");
        assert_eq!(c.column_index, 27);
        assert_eq!(c.row, 10);
    }

    #[test]
    fn round_trip_display() {
        let c: CellCoordinate = "AA42".parse().unwrap();
        assert_eq!(c.to_string(), "AA42");
    }

    #[test]
    fn from_index_round_trip() {
        let c = CellCoordinate::from_index(26, 1);
        assert_eq!(c.column, "AA");
        assert_eq!(c.column_index, 26);
        assert_eq!(c.row, 1);
    }

    #[test]
    fn invalid_no_row() {
        assert!("A".parse::<CellCoordinate>().is_err());
    }

    #[test]
    fn invalid_no_col() {
        assert!("1".parse::<CellCoordinate>().is_err());
    }

    #[test]
    fn invalid_row_zero() {
        assert!("A0".parse::<CellCoordinate>().is_err());
    }

    #[test]
    fn col_index_to_letter_basic() {
        assert_eq!(col_index_to_letter(0), "A");
        assert_eq!(col_index_to_letter(25), "Z");
        assert_eq!(col_index_to_letter(26), "AA");
        assert_eq!(col_index_to_letter(27), "AB");
        assert_eq!(col_index_to_letter(51), "AZ");
        assert_eq!(col_index_to_letter(52), "BA");
    }

    // ── CellRange ──────────────────────────

    #[test]
    fn parse_range_simple() {
        let r: CellRange = "A1:B10".parse().unwrap();
        assert!(r.sheet.is_none());
        assert_eq!(r.start, "A1".parse().unwrap());
        assert_eq!(r.end, "B10".parse().unwrap());
    }

    #[test]
    fn parse_range_with_sheet() {
        let r: CellRange = "Sheet1!A1:Z100".parse().unwrap();
        assert_eq!(r.sheet.as_deref(), Some("Sheet1"));
        assert_eq!(r.start, "A1".parse().unwrap());
        assert_eq!(r.end, "Z100".parse().unwrap());
    }

    #[test]
    fn display_range() {
        let r: CellRange = "Sheet1!A1:Z100".parse().unwrap();
        assert_eq!(r.to_string(), "Sheet1!A1:Z100");
    }

    #[test]
    fn display_range_no_sheet() {
        let r: CellRange = "A1:B10".parse().unwrap();
        assert_eq!(r.to_string(), "A1:B10");
    }

    // ── CellValue ──────────────────────────

    #[test]
    fn cell_value_serialization() {
        let v = CellValue::Number(42.0);
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("42"));
    }

    // ── Additional edge-case tests ────────

    #[test]
    fn parse_zz99() {
        let c: CellCoordinate = "ZZ99".parse().unwrap();
        assert_eq!(c.column, "ZZ");
        assert_eq!(c.column_index, 701);
        assert_eq!(c.row, 99);
    }

    #[test]
    fn parse_empty_string_errors() {
        assert!("".parse::<CellCoordinate>().is_err());
    }

    #[test]
    fn parse_digit_only_errors() {
        assert!("1A".parse::<CellCoordinate>().is_err());
    }

    #[test]
    fn parse_lowercase_normalizes() {
        let c: CellCoordinate = "a1".parse().unwrap();
        assert_eq!(c.column, "A");
    }

    #[test]
    fn from_index_various() {
        let c0 = CellCoordinate::from_index(0, 1);
        assert_eq!(c0.column, "A");
        assert_eq!(c0.column_index, 0);
        assert_eq!(c0.row, 1);

        let c25 = CellCoordinate::from_index(25, 100);
        assert_eq!(c25.column, "Z");
        assert_eq!(c25.column_index, 25);
        assert_eq!(c25.row, 100);

        let c52 = CellCoordinate::from_index(52, 1);
        assert_eq!(c52.column, "BA");
        assert_eq!(c52.column_index, 52);
    }

    #[test]
    fn parse_range_invalid_no_colon() {
        assert!("A1B10".parse::<CellRange>().is_err());
    }

    #[test]
    fn parse_range_invalid_bad_start() {
        assert!("1:B10".parse::<CellRange>().is_err());
    }

    #[test]
    fn argb_to_hex_zero_returns_none() {
        assert_eq!(argb_to_hex(0), None);
    }

    #[test]
    fn argb_to_hex_red() {
        assert_eq!(argb_to_hex(0xFF0000), Some("#FF0000".to_string()));
    }

    #[test]
    fn argb_to_hex_strips_alpha() {
        // ARGB = 0xAAFF0000 → should strip alpha AA
        assert_eq!(argb_to_hex(0xAAFF0000), Some("#FF0000".to_string()));
    }

    #[test]
    fn cell_value_empty_eq() {
        assert_eq!(CellValue::Empty, CellValue::Empty);
    }

    #[test]
    fn cell_format_default() {
        let f = CellFormat::default();
        assert!(!f.bold);
        assert!(!f.italic);
        assert!(!f.has_border);
        assert!(f.background_color.is_none());
        assert!(f.font_color.is_none());
    }
}
