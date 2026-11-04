//! Row insertion for templates with a fixed number of pre-formatted rows.
//!
//! `insert_rows` inserts empty rows below a template row and gives each one
//! the template row's styles, height and formulas, with relative row
//! references shifted to the new row. Everything below the insertion point
//! (cells, merged ranges, drawings, comments, conditional formats and formula
//! references to it) moves down, as when rows are inserted in Excel.

use crate::error::SheetsError;
use crate::file_source::FileSource;
use crate::types::InsertRowsResult;
use crate::writer::{prepare_formulas, read_umya_writable, with_xlsx_roundtrip, write_umya};

const MAX_ROW: i64 = 1_048_576;
const MAX_COLUMN: u32 = 16_384;

/// Inserts `count` rows after `after_row` (1-based) in `sheet_name`, copying
/// the format and formulas of `copy_from_row` (defaults to `after_row`).
///
/// `copy_from_row` must be at or above `after_row`, so that it does not move.
///
/// # Errors
/// - [`SheetsError::MissingField`] if `after_row`, `count` or `copy_from_row`
///   are out of range
/// - [`SheetsError::SheetNotFound`] if `sheet_name` doesn't exist
pub async fn insert_rows(
    source: &FileSource,
    output_path: Option<&str>,
    sheet_name: &str,
    after_row: u32,
    count: u32,
    copy_from_row: Option<u32>,
) -> Result<InsertRowsResult, SheetsError> {
    let template_row = copy_from_row.unwrap_or(after_row);
    if after_row == 0 || count == 0 || template_row == 0 || template_row > after_row {
        return Err(SheetsError::MissingField(format!(
            "after_row and count must be positive and copy_from_row must be between 1 and after_row \
             (after_row={after_row}, count={count}, copy_from_row={template_row})"
        )));
    }
    if i64::from(after_row) + i64::from(count) > MAX_ROW {
        return Err(SheetsError::MissingField(format!(
            "inserting {count} rows after row {after_row} exceeds the sheet's {MAX_ROW} rows"
        )));
    }

    let input_path = source.resolve().await?;
    let output = source.write_target(output_path)?;
    let mut cells_copied: u32 = 0;
    let mut formulas_copied: u32 = 0;

    with_xlsx_roundtrip(&input_path, &output, |xlsx_in, xlsx_out| {
        let mut workbook = read_umya_writable(xlsx_in)?;
        let sheet_names: Vec<String> = workbook
            .get_sheet_collection()
            .iter()
            .map(|s| s.get_name().to_string())
            .collect();
        let sheet = workbook
            .get_sheet_by_name_mut(sheet_name)
            .ok_or_else(|| SheetsError::sheet_not_found(sheet_name, &sheet_names))?;
        prepare_formulas(sheet);

        // Snapshot the template row before the insertion changes the sheet.
        let template_cells: Vec<(u32, umya_spreadsheet::Style, Option<String>)> = sheet
            .get_collection_by_row(&template_row)
            .iter()
            .map(|cell| {
                (
                    *cell.get_coordinate().get_col_num(),
                    cell.get_style().clone(),
                    cell.is_formula().then(|| cell.get_formula().to_string()),
                )
            })
            .collect();
        let template_height = sheet
            .get_row_dimension(&template_row)
            .map(|row| (*row.get_height(), *row.get_custom_height()));

        sheet.insert_new_row(&(after_row + 1), &count);

        for row in (after_row + 1)..=(after_row + count) {
            let row_offset = i64::from(row) - i64::from(template_row);
            for (column, style, formula) in &template_cells {
                let cell = sheet.get_cell_mut((*column, row));
                cell.set_style(style.clone());
                if let Some(formula) = formula {
                    cell.set_formula(shift_formula_rows(formula, row_offset));
                    formulas_copied += 1;
                }
                cells_copied += 1;
            }
            if let Some((height, custom)) = template_height {
                if height > 0.0 {
                    let dimension = sheet.get_row_dimension_mut(&row);
                    dimension.set_height(height);
                    dimension.set_custom_height(custom);
                }
            }
        }

        write_umya(&workbook, xlsx_out)
    })
    .await?;

    tracing::info!(
        "insert_rows: {} rows after row {} (format of row {}) in '{}' → {:?}",
        count,
        after_row,
        template_row,
        sheet_name,
        output
    );

    Ok(InsertRowsResult {
        sheet_name: sheet_name.to_string(),
        output_path: output.display().to_string(),
        inserted_rows: count,
        first_new_row: after_row + 1,
        last_new_row: after_row + count,
        cells_copied,
        formulas_copied,
    })
}

/// Shifts the relative row of every A1 reference in `formula` by `offset`
/// rows. Absolute rows (`$7`), text literals and quoted sheet names are kept.
/// A reference shifted above row 1 becomes `#REF!`.
///
/// ```
/// use sheets_mcp::rows::shift_formula_rows;
/// assert_eq!(shift_formula_rows("SUM(R7,U7,X7)", 2), "SUM(R9,U9,X9)");
/// assert_eq!(shift_formula_rows("A$1*B7", 1), "A$1*B8");
/// ```
pub fn shift_formula_rows(formula: &str, offset: i64) -> String {
    let chars: Vec<char> = formula.chars().collect();
    let mut result = String::with_capacity(formula.len());
    let mut index = 0;
    while index < chars.len() {
        let current = chars[index];
        if current == '"' || current == '\'' {
            let end = quoted_end(&chars, index, current);
            result.extend(&chars[index..end]);
            index = end;
            continue;
        }
        let starts_token = index == 0 || !is_name_char(chars[index - 1]);
        if starts_token {
            if let Some((end, reference)) = parse_reference(&chars, index) {
                result.push_str(&reference.shifted(offset));
                index = end;
                continue;
            }
        }
        result.push(current);
        index += 1;
    }
    result
}

/// Index just past the closing quote; a doubled quote is an escaped quote.
fn quoted_end(chars: &[char], start: usize, quote: char) -> usize {
    let mut index = start + 1;
    while index < chars.len() {
        if chars[index] == quote {
            if chars.get(index + 1) == Some(&quote) {
                index += 2;
                continue;
            }
            return index + 1;
        }
        index += 1;
    }
    chars.len()
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$'
}

struct CellReference {
    column_lock: bool,
    column: String,
    row_lock: bool,
    row: i64,
}

impl CellReference {
    fn shifted(&self, offset: i64) -> String {
        let row = if self.row_lock {
            self.row
        } else {
            self.row + offset
        };
        if !(1..=MAX_ROW).contains(&row) {
            return "#REF!".to_string();
        }
        format!(
            "{}{}{}{}",
            if self.column_lock { "$" } else { "" },
            self.column,
            if self.row_lock { "$" } else { "" },
            row
        )
    }
}

/// Parses `[$]COL[$]ROW` at `start`. Rejects names (followed by a name
/// character), function calls (followed by `(`) and sheet prefixes
/// (followed by `!`).
fn parse_reference(chars: &[char], start: usize) -> Option<(usize, CellReference)> {
    let mut index = start;
    let column_lock = chars.get(index) == Some(&'$');
    if column_lock {
        index += 1;
    }
    let column_start = index;
    while index < chars.len() && chars[index].is_ascii_alphabetic() && index - column_start < 3 {
        index += 1;
    }
    if index == column_start {
        return None;
    }
    let column: String = chars[column_start..index].iter().collect();
    if column_number(&column)? > MAX_COLUMN {
        return None;
    }
    let row_lock = chars.get(index) == Some(&'$');
    if row_lock {
        index += 1;
    }
    let row_start = index;
    while index < chars.len() && chars[index].is_ascii_digit() {
        index += 1;
    }
    if index == row_start || index - row_start > 7 {
        return None;
    }
    if let Some(next) = chars.get(index) {
        if is_name_char(*next) || *next == '(' || *next == '!' {
            return None;
        }
    }
    let row: i64 = chars[row_start..index]
        .iter()
        .collect::<String>()
        .parse()
        .ok()?;
    if row == 0 {
        return None;
    }
    Some((
        index,
        CellReference {
            column_lock,
            column,
            row_lock,
            row,
        },
    ))
}

fn column_number(letters: &str) -> Option<u32> {
    letters.chars().try_fold(0u32, |total, letter| {
        let value = letter.to_ascii_uppercase() as u32;
        (b'A' as u32..=b'Z' as u32)
            .contains(&value)
            .then(|| total * 26 + (value - b'A' as u32 + 1))
    })
}

#[cfg(test)]
mod tests {
    use super::shift_formula_rows;

    #[test]
    fn shifts_relative_rows() {
        assert_eq!(shift_formula_rows("O11*0.19", 1), "O12*0.19");
        assert_eq!(shift_formula_rows("(S11*T11)", 3), "(S14*T14)");
        assert_eq!(
            shift_formula_rows("SUM(R11,U11,X11)", 2),
            "SUM(R13,U13,X13)"
        );
        assert_eq!(shift_formula_rows("SUM(A2:A11)", 1), "SUM(A3:A12)");
    }

    #[test]
    fn keeps_absolute_rows_text_and_names() {
        assert_eq!(shift_formula_rows("$A$1+A$2+$B3", 5), "$A$1+A$2+$B8");
        assert_eq!(
            shift_formula_rows("IF(A2=\"B7\",1,0)", 1),
            "IF(A3=\"B7\",1,0)"
        );
        assert_eq!(
            shift_formula_rows("'Hoja 1'!A2+LOG10(B2)", 1),
            "'Hoja 1'!A3+LOG10(B3)"
        );
        assert_eq!(
            shift_formula_rows("Sheet2!C4*TAX_2024", 2),
            "Sheet2!C6*TAX_2024"
        );
        assert_eq!(shift_formula_rows("1.5E10*A1", 1), "1.5E10*A2");
    }

    #[test]
    fn references_above_row_one_become_ref_errors() {
        assert_eq!(shift_formula_rows("A1+B2", -1), "#REF!+B1");
    }
}
