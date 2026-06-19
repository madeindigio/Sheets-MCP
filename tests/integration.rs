//! Integration tests for sheets_mcp.
//!
//! These tests exercise the full read → modify → read round-trip,
//! verifying formula safety, format preservation, and array paste.

use sheets_mcp::file_source::FileSource;
use sheets_mcp::reader::{count_sheet_rows, get_sheet_range, read_structure};
use sheets_mcp::types::CellValue;
use sheets_mcp::writer::{update_cell, update_cells};
use std::path::PathBuf;
use umya_spreadsheet::new_file;

// ── Helpers ──────────────────────────────────

fn test_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("sheets_mcp_integration");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Create a workbook with formulas, styles, and data.
///
/// Layout:
///   A1="Name" B1="Score" C1="Grade"  (header row)
///   A2="Alice" B2=95      C2=IF(B2>=90,"A","B")  (formula)
///   A3="Bob"   B3=78      C3=IF(B3>=90,"A","B")  (formula)
///   A4="Eve"   B4=88      C4=IF(B4>=90,"A","B")  (formula)
///
/// Formatting:
///   A1: bold font
///   B1: red background (#FF0000)
///   C1: has a thin border
fn create_full_workbook(name: &str) -> PathBuf {
    let mut book = new_file();
    let ws = book.get_sheet_mut(&0).unwrap();

    // ── Header row with formatting ──
    let a1 = ws.get_cell_mut("A1");
    a1.set_value("Name");
    a1.get_style_mut().get_font_mut().set_bold(true);

    let b1 = ws.get_cell_mut("B1");
    b1.set_value("Score");
    b1.get_style_mut()
        .get_fill_mut()
        .get_pattern_fill_mut()
        .get_background_color_mut()
        .set_argb("FFFF0000"); // red

    let c1 = ws.get_cell_mut("C1");
    c1.set_value("Grade");
    c1.get_style_mut()
        .get_borders_mut()
        .get_left_border_mut()
        .set_border_style("thin");

    // ── Data rows ──
    ws.get_cell_mut("A2").set_value("Alice");
    ws.get_cell_mut("B2").set_value_number(95.0);
    ws.get_cell_mut("C2").set_value("=IF(B2>=90,\"A\",\"B\")");

    ws.get_cell_mut("A3").set_value("Bob");
    ws.get_cell_mut("B3").set_value_number(78.0);
    ws.get_cell_mut("C3").set_value("=IF(B3>=90,\"A\",\"B\")");

    ws.get_cell_mut("A4").set_value("Eve");
    ws.get_cell_mut("B4").set_value_number(88.0);
    ws.get_cell_mut("C4").set_value("=IF(B4>=90,\"A\",\"B\")");

    let path = test_dir().join(format!("{name}.xlsx"));
    umya_spreadsheet::writer::xlsx::write(&book, &path).unwrap();
    path
}

fn source(path: &std::path::Path) -> FileSource {
    FileSource::Path {
        file_path: path.display().to_string(),
    }
}

// ── Tests ────────────────────────────────────

#[tokio::test]
async fn update_cell_changes_value() {
    let path = create_full_workbook("intg_update_cell");
    let src = source(&path);

    // Update B2 from 95 to 50.
    let result = update_cell(&src, None, "Sheet1", "B2", "50", "number")
        .await
        .unwrap();
    assert_eq!(result.updated_count, 1);

    // Re-read and verify.
    let range = get_sheet_range(&src, "Sheet1", "B2:B2", false)
        .await
        .unwrap();
    assert_eq!(range.rows[0][0].value, CellValue::Number(50.0));
}

#[tokio::test]
async fn update_cell_preserves_formula() {
    let path = create_full_workbook("intg_formula_safety");
    let src = source(&path);

    // Update B2 from 95 → 50.
    update_cell(&src, None, "Sheet1", "B2", "50", "number")
        .await
        .unwrap();

    // Re-read with umya to check C2 still has the formula text.
    let wb = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
    let ws = wb.get_sheet_by_name("Sheet1").unwrap();
    let c2 = ws.get_cell("C2").unwrap();
    assert_eq!(c2.get_value(), "=IF(B2>=90,\"A\",\"B\")");

    // Also verify B2 was updated.
    let b2 = ws.get_cell("B2").unwrap();
    assert_eq!(b2.get_value(), "50");
}

#[tokio::test]
async fn update_cells_pastes_2d_array() {
    let path = create_full_workbook("intg_array_paste");
    let src = source(&path);

    let data = vec![
        vec!["X".to_string(), "100".to_string()],
        vec!["Y".to_string(), "200".to_string()],
        vec!["Z".to_string(), "300".to_string()],
    ];

    let result = update_cells(&src, None, "Sheet1", "E2", &data)
        .await
        .unwrap();
    assert_eq!(result.updated_count, 6);
    assert_eq!(result.rows, Some(3));
    assert_eq!(result.columns, Some(2));

    // Re-read and verify.
    // Note: calamine auto-detects "100" as a number, so we check both possible representations.
    let range = get_sheet_range(&src, "Sheet1", "E2:F4", false)
        .await
        .unwrap();

    assert_eq!(range.rows[0][0].value, CellValue::String("X".to_string()));
    // "100" set via set_value may be read as Number by calamine
    assert!(
        range.rows[0][1].value == CellValue::String("100".to_string())
            || range.rows[0][1].value == CellValue::Number(100.0),
        "Expected '100' string or number, got {:?}",
        range.rows[0][1].value
    );
    assert_eq!(range.rows[1][0].value, CellValue::String("Y".to_string()));
    assert!(
        range.rows[2][1].value == CellValue::String("300".to_string())
            || range.rows[2][1].value == CellValue::Number(300.0),
        "Expected '300' string or number, got {:?}",
        range.rows[2][1].value
    );
}

#[tokio::test]
async fn update_cells_preserves_formulas_in_range() {
    let path = create_full_workbook("intg_array_formula");
    let src = source(&path);

    // Paste values into B2:B3 (overwriting numeric cells, but C2:C3 have formulas).
    let data = vec![
        vec!["5".to_string()],
        vec!["95".to_string()],
    ];
    update_cells(&src, None, "Sheet1", "B2", &data)
        .await
        .unwrap();

    // Check formulas are intact.
    let wb = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
    let ws = wb.get_sheet_by_name("Sheet1").unwrap();
    assert_eq!(ws.get_cell("C2").unwrap().get_value(), "=IF(B2>=90,\"A\",\"B\")");
    assert_eq!(ws.get_cell("C3").unwrap().get_value(), "=IF(B3>=90,\"A\",\"B\")");
}

#[tokio::test]
async fn format_preserved_after_update_cell() {
    let path = create_full_workbook("intg_format_preserve");
    let src = source(&path);

    // Verify format before update.
    let range_before = get_sheet_range(&src, "Sheet1", "A1:C1", true)
        .await
        .unwrap();
    let fmt_a1_before = range_before.rows[0][0].format.as_ref().unwrap().clone();
    let fmt_b1_before = range_before.rows[0][1].format.as_ref().unwrap().clone();
    let fmt_c1_before = range_before.rows[0][2].format.as_ref().unwrap().clone();

    assert!(fmt_a1_before.bold);
    assert!(fmt_b1_before.background_color.is_some());
    assert!(fmt_c1_before.has_border);

    // Update A1 value (should not touch formatting).
    update_cell(&src, None, "Sheet1", "A1", "Changed", "string")
        .await
        .unwrap();

    // Verify format after update.
    let range_after = get_sheet_range(&src, "Sheet1", "A1:C1", true)
        .await
        .unwrap();
    let fmt_a1_after = range_after.rows[0][0].format.as_ref().unwrap();
    let fmt_b1_after = range_after.rows[0][1].format.as_ref().unwrap();
    let fmt_c1_after = range_after.rows[0][2].format.as_ref().unwrap();

    assert!(fmt_a1_after.bold, "Bold should be preserved on A1");
    assert_eq!(
        fmt_b1_after.background_color, fmt_b1_before.background_color,
        "Background color should be preserved on B1"
    );
    assert_eq!(
        fmt_c1_after.has_border, fmt_c1_before.has_border,
        "Border should be preserved on C1"
    );

    // Verify value was actually updated.
    assert_eq!(
        range_after.rows[0][0].value,
        CellValue::String("Changed".to_string())
    );
}

#[tokio::test]
async fn read_write_roundtrip() {
    let path = create_full_workbook("intg_roundtrip");
    let src = source(&path);

    // Read structure.
    let structure = read_structure(&src, 10).await.unwrap();
    assert_eq!(structure.sheets.len(), 1);
    assert_eq!(structure.sheets[0].total_rows, 4);
    assert_eq!(structure.sheets[0].total_columns, 3);

    // Count rows.
    let count = count_sheet_rows(&src, "Sheet1").await.unwrap();
    assert_eq!(count.total_rows, 4);
    assert_eq!(count.total_columns, 3);

    // Update and verify.
    update_cell(&src, None, "Sheet1", "A2", "Zara", "string")
        .await
        .unwrap();

    let range = get_sheet_range(&src, "Sheet1", "A2:A2", false)
        .await
        .unwrap();
    assert_eq!(range.rows[0][0].value, CellValue::String("Zara".to_string()));
}

#[tokio::test]
async fn write_target_respects_output_path() {
    let path = create_full_workbook("intg_output_path");
    let src = source(&path);

    let output = test_dir().join("intg_output_path_result.xlsx");

    update_cell(
        &src,
        Some(output.to_str().unwrap()),
        "Sheet1",
        "A1",
        "Overwritten",
        "string",
    )
    .await
    .unwrap();

    // Verify original is unchanged.
    let orig_range = get_sheet_range(&src, "Sheet1", "A1:A1", false)
        .await
        .unwrap();
    assert_eq!(
        orig_range.rows[0][0].value,
        CellValue::String("Name".to_string())
    );

    // Verify output has the change.
    let out_src = source(&output);
    let out_range = get_sheet_range(&out_src, "Sheet1", "A1:A1", false)
        .await
        .unwrap();
    assert_eq!(
        out_range.rows[0][0].value,
        CellValue::String("Overwritten".to_string())
    );
}
