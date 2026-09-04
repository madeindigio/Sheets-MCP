//! Write operations for Excel workbooks.
//!
//! # Formula & Format Safety
//!
//! This is the **most critical invariant** of the project:
//!
//! umya-spreadsheet stores each cell as independent XML elements:
//! - `<v>` — value
//! - `<f>` — formula (e.g., `=A1+B1`)
//! - `s=""` — style index (references a style definition)
//!
//! `cell.set_value("42")` only writes to `<v>`. It never touches `<f>` or `s=""`.
//!
//! **Therefore**: updating any cell preserves ALL formulas and ALL formatting
//! in the entire workbook. This is by design, not by accident.

use std::path::Path;

use crate::error::SheetsError;
use crate::file_source::FileSource;
use crate::types::{is_legacy_xls, CellCoordinate, UpdateResult};

// ──────────────────────────────────────────────
// update_cell
// ──────────────────────────────────────────────

/// Updates a single cell's value in an Excel workbook.
///
/// Only modifies the cell's value element (`<v>`).
/// Preserves formulas (`<f>`) and formatting (`s=""` attribute)
/// in ALL cells — this is the core safety guarantee.
///
/// # Arguments
/// * `source` - Local file path or remote URL to the workbook
/// * `output_path` - Where to save (required for URLs, optional for local files)
/// * `sheet_name` - Name of the sheet containing the cell
/// * `coordinate` - Cell coordinate string (e.g. `"B5"`)
/// * `value` - New value as a string
/// * `value_type` - One of: `"number"`, `"bool"`, or `"string"` (default)
///
/// # Guarantees
/// - All formulas in the workbook are preserved
/// - All cell formatting (colors, fonts, borders) is preserved
/// - Only the target cell's value XML element is modified
///
/// # Errors
/// - [`SheetsError::SheetNotFound`] if sheet_name doesn't exist
/// - [`SheetsError::InvalidCoordinate`] if coordinate can't be parsed
/// - [`SheetsError::MissingOutputPath`] if source is a URL and no output_path
pub async fn update_cell(
    source: &FileSource,
    output_path: Option<&str>,
    sheet_name: &str,
    coordinate: &str,
    value: &str,
    value_type: &str, // "string", "number", "bool"
) -> Result<UpdateResult, SheetsError> {
    let input_path = source.resolve().await?;
    let output = source.write_target(output_path)?;

    let coord: CellCoordinate = coordinate.parse()?;
    let cell_ref = coord.to_string();

    with_xlsx_roundtrip(&input_path, &output, |xlsx_in, xlsx_out| {
        let mut workbook = read_umya_writable(xlsx_in)?;

        // Collect sheet names before mutable borrow.
        let sheet_names: Vec<String> = workbook
            .get_sheet_collection()
            .iter()
            .map(|s| s.get_name().to_string())
            .collect();

        // Get mutable sheet.
        let sheet = workbook
            .get_sheet_by_name_mut(sheet_name)
            .ok_or_else(|| SheetsError::sheet_not_found(sheet_name, &sheet_names))?;

        // Get mutable cell and set value.
        let cell = sheet.get_cell_mut(cell_ref.as_str());
        set_cell_value(cell, value, value_type)?;

        // Write back.
        write_umya(&workbook, xlsx_out)
    })
    .await?;

    tracing::info!(
        "update_cell: set {} = {} ({}) in '{}' → {:?}",
        coord, value, value_type, sheet_name, output
    );

    Ok(UpdateResult {
        updated_count: 1,
        sheet_name: sheet_name.to_string(),
        output_path: output.display().to_string(),
        start: Some(cell_ref),
        rows: Some(1),
        columns: Some(1),
    })
}

// ──────────────────────────────────────────────
// update_cells
// ──────────────────────────────────────────────

/// Pastes a 2D array of string values starting at a destination cell.
///
/// Only modifies each target cell's value element (`<v>`).
/// Preserves formulas and formatting in ALL cells — including ones
/// that are being overwritten.
///
/// # Arguments
/// * `source` - Local file path or remote URL to the workbook
/// * `output_path` - Where to save (required for URLs, optional for local files)
/// * `sheet_name` - Name of the sheet
/// * `destination` - Top-left cell coordinate (e.g. `"B2"`)
/// * `data` - 2D array of string values (row-major)
///
/// # Guarantees
/// Same as [`update_cell`] — all formulas and formatting preserved.
///
/// # Errors
/// - [`SheetsError::MissingField`] if `data` is empty
/// - [`SheetsError::SheetNotFound`] if sheet_name doesn't exist
/// - [`SheetsError::InvalidCoordinate`] if destination can't be parsed
pub async fn update_cells(
    source: &FileSource,
    output_path: Option<&str>,
    sheet_name: &str,
    destination: &str,
    data: &[Vec<String>],
) -> Result<UpdateResult, SheetsError> {
    if data.is_empty() {
        return Err(SheetsError::MissingField(
            "data must not be empty".to_string(),
        ));
    }

    let input_path = source.resolve().await?;
    let output = source.write_target(output_path)?;

    // Parse destination coordinate.
    let start: CellCoordinate = destination.parse()?;
    let start_col = start.column_index;
    let start_row = start.row;

    let num_rows = data.len() as u32;
    let num_cols = data.iter().map(|r| r.len() as u32).max().unwrap_or(0);
    let mut written: u32 = 0;

    with_xlsx_roundtrip(&input_path, &output, |xlsx_in, xlsx_out| {
        let mut workbook = read_umya_writable(xlsx_in)?;

        // Collect sheet names before mutable borrow.
        let sheet_names: Vec<String> = workbook
            .get_sheet_collection()
            .iter()
            .map(|s| s.get_name().to_string())
            .collect();

        // Get mutable sheet.
        let sheet = workbook
            .get_sheet_by_name_mut(sheet_name)
            .ok_or_else(|| SheetsError::sheet_not_found(sheet_name, &sheet_names))?;

        for (r, row_data) in data.iter().enumerate() {
            for (c, value_str) in row_data.iter().enumerate() {
                let coord =
                    CellCoordinate::from_index(start_col + c as u32, start_row + r as u32);
                let cell_ref = coord.to_string();
                let cell = sheet.get_cell_mut(cell_ref.as_str());

                // All values from data are treated as strings (raw paste).
                // If the value is empty, we still set it (clears the cell value).
                cell.set_value(value_str.as_str());
                written += 1;
            }
        }

        // Write back.
        write_umya(&workbook, xlsx_out)
    })
    .await?;

    tracing::info!(
        "update_cells: pasted {}x{} at {} in '{}' → {:?}",
        num_rows, num_cols, destination, sheet_name, output
    );

    Ok(UpdateResult {
        updated_count: written,
        sheet_name: sheet_name.to_string(),
        output_path: output.display().to_string(),
        start: Some(destination.to_string()),
        rows: Some(num_rows),
        columns: Some(num_cols),
    })
}

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

/// Runs `f` against a working `.xlsx` pair, transparently round-tripping
/// through LibreOffice when `input_path` is a legacy `.xls` file.
///
/// - `.xlsx` input: `f` is called directly with `(input_path, output_path)`.
/// - `.xls` input: `input_path` is converted to a temporary `.xlsx` via
///   `soffice`, `f` is called against that temp file (read and write to the
///   same path), and the result is converted back to `.xls` and copied to
///   `output_path`. Temp files are cleaned up in all cases.
///
/// `f` performs the actual umya read-modify-write and must not assume
/// anything about the final output format.
async fn with_xlsx_roundtrip<F>(
    input_path: &Path,
    output_path: &Path,
    f: F,
) -> Result<(), SheetsError>
where
    F: FnOnce(&Path, &Path) -> Result<(), SheetsError>,
{
    if !is_legacy_xls(input_path) {
        return f(input_path, output_path);
    }

    let (temp_dir, xlsx_path) = convert_xls_to_xlsx(input_path).await?;

    let result = f(&xlsx_path, &xlsx_path);
    let final_result = match result {
        Ok(()) => convert_xlsx_to_xls(&xlsx_path, output_path).await,
        Err(e) => Err(e),
    };

    let _ = std::fs::remove_dir_all(&temp_dir);

    final_result
}

/// A unique, filesystem-safe suffix for temp dirs / LibreOffice profiles.
///
/// Avoids collisions between concurrent MCP tool calls without pulling in
/// a `uuid` dependency.
fn unique_suffix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}_{}", std::process::id(), nanos)
}

/// Converts a legacy `.xls` file to `.xlsx` via a headless LibreOffice
/// subprocess, returning `(temp_dir, xlsx_path)`. The caller is responsible
/// for removing `temp_dir` once done.
async fn convert_xls_to_xlsx(
    xls_path: &Path,
) -> Result<(std::path::PathBuf, std::path::PathBuf), SheetsError> {
    let suffix = unique_suffix();
    let temp_dir = std::env::temp_dir().join(format!("sheets_mcp_lo_{suffix}"));
    let profile_dir =
        std::env::temp_dir().join(format!("sheets_mcp_lo_profile_{suffix}"));
    std::fs::create_dir_all(&temp_dir)?;

    let stem = xls_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("workbook");
    let input_copy = temp_dir.join(format!("{stem}.xls"));
    std::fs::copy(xls_path, &input_copy)?;

    let result = run_soffice(&[
        "--headless".to_string(),
        format!("-env:UserInstallation=file://{}", profile_dir.display()),
        "--convert-to".to_string(),
        "xlsx".to_string(),
        "--outdir".to_string(),
        temp_dir.display().to_string(),
        input_copy.display().to_string(),
    ])
    .await;

    let _ = std::fs::remove_dir_all(&profile_dir);

    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(e);
    }

    let xlsx_path = temp_dir.join(format!("{stem}.xlsx"));
    if !xlsx_path.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(SheetsError::ExternalConversionFailed(
            "soffice did not produce the expected .xlsx output".to_string(),
        ));
    }

    Ok((temp_dir, xlsx_path))
}

/// Converts an `.xlsx` file back to legacy `.xls` via a headless
/// LibreOffice subprocess and copies the result to `final_output`.
async fn convert_xlsx_to_xls(
    xlsx_path: &Path,
    final_output: &Path,
) -> Result<(), SheetsError> {
    let suffix = unique_suffix();
    let temp_dir = std::env::temp_dir().join(format!("sheets_mcp_lo_out_{suffix}"));
    let profile_dir =
        std::env::temp_dir().join(format!("sheets_mcp_lo_profile_{suffix}"));
    std::fs::create_dir_all(&temp_dir)?;

    let stem = xlsx_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("workbook");

    let result = run_soffice(&[
        "--headless".to_string(),
        format!("-env:UserInstallation=file://{}", profile_dir.display()),
        "--convert-to".to_string(),
        "xls:MS Excel 97".to_string(),
        "--outdir".to_string(),
        temp_dir.display().to_string(),
        xlsx_path.display().to_string(),
    ])
    .await;

    let _ = std::fs::remove_dir_all(&profile_dir);

    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(e);
    }

    let converted_xls = temp_dir.join(format!("{stem}.xls"));
    if !converted_xls.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(SheetsError::ExternalConversionFailed(
            "soffice did not produce the expected .xls output".to_string(),
        ));
    }

    if let Some(parent) = final_output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(&converted_xls, final_output)?;

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

/// Runs `soffice` with the given arguments, enforcing a 30s timeout and
/// mapping spawn/timeout/non-zero-exit failures to
/// [`SheetsError::ExternalConversionFailed`] with an actionable message.
async fn run_soffice(args: &[String]) -> Result<(), SheetsError> {
    run_soffice_bin("soffice", args).await
}

/// Same as [`run_soffice`] but with an injectable binary name, so tests can
/// exercise the "LibreOffice not installed" path without mutating the
/// process-wide `PATH`.
async fn run_soffice_bin(bin: &str, args: &[String]) -> Result<(), SheetsError> {
    let spawn = tokio::process::Command::new(bin)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    let output = match tokio::time::timeout(std::time::Duration::from_secs(30), spawn).await {
        Err(_) => {
            return Err(SheetsError::ExternalConversionFailed(
                "LibreOffice (soffice) conversion timed out after 30s".to_string(),
            ))
        }
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(SheetsError::ExternalConversionFailed(
                "Writing legacy .xls requires LibreOffice (`soffice`) to be installed and on PATH"
                    .to_string(),
            ))
        }
        Ok(Err(e)) => {
            return Err(SheetsError::ExternalConversionFailed(format!(
                "failed to run soffice: {e}"
            )))
        }
        Ok(Ok(output)) => output,
    };

    if !output.status.success() {
        return Err(SheetsError::ExternalConversionFailed(format!(
            "soffice exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(())
}

/// Read an `.xlsx` file using umya-spreadsheet for writing.
///
/// Returns the full spreadsheet DOM. The caller modifies cells
/// and then writes back with [`write_umya`].
fn read_umya_writable(
    path: &Path,
) -> Result<umya_spreadsheet::Spreadsheet, SheetsError> {
    umya_spreadsheet::reader::xlsx::read(path)
        .map_err(|e| SheetsError::Spreadsheet(e.to_string()))
}

/// Write the workbook back to disk using umya-spreadsheet.
///
/// Ensures the parent directory exists before writing.
fn write_umya(
    workbook: &umya_spreadsheet::Spreadsheet,
    path: &Path,
) -> Result<(), SheetsError> {
    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    umya_spreadsheet::writer::xlsx::write(workbook, path)
        .map_err(|e| SheetsError::Spreadsheet(e.to_string()))
}


/// Set a cell value based on the declared type.
///
/// - `"number"` → parses as `f64`, calls `set_value_number`
/// - `"bool"` → parses as `bool`, calls `set_value_bool`
/// - anything else → calls `set_value` (string)
fn set_cell_value(
    cell: &mut umya_spreadsheet::structs::Cell,
    value: &str,
    value_type: &str,
) -> Result<(), SheetsError> {
    match value_type {
        "number" => {
            let parsed: f64 = value.parse().map_err(|_| {
                SheetsError::InvalidCoordinate(format!(
                    "Cannot parse '{value}' as a number"
                ))
            })?;
            cell.set_value_number(parsed);
        }
        "bool" => {
            let parsed = match value.to_lowercase().as_str() {
                "true" => true,
                "false" => false,
                _ => {
                    return Err(SheetsError::InvalidCoordinate(
                        format!(
                            "Cannot parse '{value}' as a boolean (expected 'true' or 'false')"
                        ),
                    ))
                }
            };
            cell.set_value_bool(parsed);
        }
        _ => {
            // Default: treat as string.
            cell.set_value(value);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a test workbook with a formula + styled cell.
    fn create_test_workbook(path: &Path) {
        let mut wb = umya_spreadsheet::new_file();

        // Sheet1: set up a formula scenario.
        // A1=10, B1=20, C1=A1+B1 (formula)
        {
            let sheet = wb.get_sheet_by_name_mut("Sheet1")
                .expect("Sheet1 exists");

            sheet.get_cell_mut("A1").set_value_number(10.0);
            sheet.get_cell_mut("B1").set_value_number(20.0);
            sheet.get_cell_mut("C1").set_value("=A1+B1");
        }

        // Write to disk.
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        umya_spreadsheet::writer::xlsx::write(&wb, path)
            .expect("Failed to write test workbook");
    }

    #[test]
    fn test_update_cell_number() {
        let dir = std::env::temp_dir().join("sheets_mcp_test");
        let path = dir.join("test_update_cell.xlsx");
        create_test_workbook(&path);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let source = FileSource::Path {
            file_path: path.display().to_string(),
        };

        let result = rt.block_on(update_cell(
            &source,
            None,
            "Sheet1",
            "A1",
            "42",
            "number",
        ));

        assert!(result.is_ok());
        let res = result.unwrap();
        assert_eq!(res.updated_count, 1);
        assert_eq!(res.sheet_name, "Sheet1");
    }

    #[test]
    fn test_formula_preserved_after_cell_update() {
        let dir = std::env::temp_dir().join("sheets_mcp_test");
        let path = dir.join("test_formula_preserved.xlsx");
        create_test_workbook(&path);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let source = FileSource::Path {
            file_path: path.display().to_string(),
        };

        // Update A1 from 10 → 5.
        rt.block_on(update_cell(
            &source,
            None,
            "Sheet1",
            "A1",
            "5",
            "number",
        ))
        .unwrap();

        // Re-read the workbook and check C1 still has the formula.
        let wb = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
        let sheet = wb.get_sheet_by_name("Sheet1").unwrap();
        let c1 = sheet.get_cell("C1").unwrap();
        assert_eq!(c1.get_value(), "=A1+B1");

        // Clean up.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_update_cells_array() {
        let dir = std::env::temp_dir().join("sheets_mcp_test");
        let path = dir.join("test_update_cells.xlsx");
        create_test_workbook(&path);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let source = FileSource::Path {
            file_path: path.display().to_string(),
        };

        let data = vec![
            vec!["x".to_string(), "y".to_string()],
            vec!["z".to_string(), "w".to_string()],
        ];

        let result = rt.block_on(update_cells(
            &source,
            None,
            "Sheet1",
            "B2",
            &data,
        ));

        assert!(result.is_ok());
        let res = result.unwrap();
        assert_eq!(res.updated_count, 4);
        assert_eq!(res.rows, Some(2));
        assert_eq!(res.columns, Some(2));

        // Verify values in the saved workbook.
        let wb = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
        let sheet = wb.get_sheet_by_name("Sheet1").unwrap();
        assert_eq!(sheet.get_cell("B2").unwrap().get_value(), "x");
        assert_eq!(sheet.get_cell("C2").unwrap().get_value(), "y");
        assert_eq!(sheet.get_cell("B3").unwrap().get_value(), "z");
        assert_eq!(sheet.get_cell("C3").unwrap().get_value(), "w");

        // Verify formula in C1 is still intact.
        assert_eq!(sheet.get_cell("C1").unwrap().get_value(), "=A1+B1");

        // Clean up.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_invalid_coordinate_errors() {
        let dir = std::env::temp_dir().join("sheets_mcp_test");
        let path = dir.join("test_invalid_coord.xlsx");
        create_test_workbook(&path);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let source = FileSource::Path {
            file_path: path.display().to_string(),
        };

        // Invalid coordinate.
        let result = rt.block_on(update_cell(
            &source,
            None,
            "Sheet1",
            "1A", // reversed — no column letters
            "42",
            "number",
        ));

        assert!(result.is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_sheet_not_found_errors() {
        let dir = std::env::temp_dir().join("sheets_mcp_test");
        let path = dir.join("test_sheet_not_found.xlsx");
        create_test_workbook(&path);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let source = FileSource::Path {
            file_path: path.display().to_string(),
        };

        let result = rt.block_on(update_cell(
            &source,
            None,
            "NonExistent",
            "A1",
            "42",
            "number",
        ));

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SheetsError::SheetNotFound { .. }));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_file_not_found_errors() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let source = FileSource::Path {
            file_path: "/tmp/nonexistent_file_abc123.xlsx".into(),
        };

        let result = rt.block_on(update_cell(
            &source,
            None,
            "Sheet1",
            "A1",
            "42",
            "number",
        ));

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SheetsError::FileNotFound(_)));
    }

    #[test]
    fn test_run_soffice_missing_binary_errors_clearly() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_soffice_bin(
            "sheets_mcp_definitely_not_a_real_binary_xyz",
            &[],
        ));

        match result {
            Err(SheetsError::ExternalConversionFailed(msg)) => {
                assert!(
                    msg.contains("LibreOffice") && msg.contains("PATH"),
                    "expected an actionable message, got: {msg}"
                );
            }
            other => panic!("expected ExternalConversionFailed, got: {other:?}"),
        }
    }
}
