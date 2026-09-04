//! Integration tests for writing to legacy `.xls` (BIFF8/OLE2) workbooks.
//!
//! `update_cell`/`update_cells` round-trip `.xls` inputs through a headless
//! LibreOffice conversion (see `writer::with_xlsx_roundtrip`). These tests
//! require `soffice` to be installed and on `PATH`; they no-op with a
//! message if it isn't, since that's a system dependency rather than a
//! Cargo dependency.

use sheets_mcp::file_source::FileSource;
use sheets_mcp::reader::get_sheet_range;
use sheets_mcp::types::CellValue;
use sheets_mcp::writer::{update_cell, update_cells};
use std::path::PathBuf;
use std::process::Command;

fn test_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("sheets_mcp_xls_integration");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn soffice_available() -> bool {
    Command::new("soffice")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Builds a small workbook with umya and converts it to legacy `.xls` via
/// LibreOffice, to use as a test fixture.
fn create_legacy_xls_fixture(name: &str) -> PathBuf {
    let mut book = umya_spreadsheet::new_file();
    {
        let ws = book.get_sheet_mut(&0).unwrap();
        ws.get_cell_mut("A1").set_value("Name");
        ws.get_cell_mut("B1").set_value_number(10.0);
        ws.get_cell_mut("A2").set_value("Alice");
        ws.get_cell_mut("B2").set_value_number(95.0);
    }

    let dir = test_dir();
    let xlsx_path = dir.join(format!("{name}.xlsx"));
    umya_spreadsheet::writer::xlsx::write(&book, &xlsx_path).unwrap();

    let status = Command::new("soffice")
        .args(["--headless", "--convert-to", "xls:MS Excel 97", "--outdir"])
        .arg(&dir)
        .arg(&xlsx_path)
        .status()
        .expect("soffice must be installed to build the .xls fixture");
    assert!(status.success(), "soffice fixture conversion failed");

    dir.join(format!("{name}.xls"))
}

fn source(path: &std::path::Path) -> FileSource {
    FileSource::Path {
        file_path: path.display().to_string(),
    }
}

/// Legacy `.xls` (OLE2/CFB) files start with this magic number.
/// A `.xlsx` (ZIP) file would start with `PK\x03\x04` instead.
const OLE2_MAGIC: [u8; 4] = [0xD0, 0xCF, 0x11, 0xE0];

#[tokio::test]
async fn update_cell_roundtrips_legacy_xls() {
    if !soffice_available() {
        eprintln!("skipping update_cell_roundtrips_legacy_xls: soffice not installed");
        return;
    }
    let path = create_legacy_xls_fixture("xls_update_cell");
    let src = source(&path);

    let result = update_cell(&src, None, "Sheet1", "B2", "42", "number")
        .await
        .unwrap();
    assert_eq!(result.updated_count, 1);

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        &bytes[0..4],
        &OLE2_MAGIC,
        "output must remain a real OLE2 .xls file, not a renamed .xlsx"
    );

    let range = get_sheet_range(&src, "Sheet1", "B2:B2", false)
        .await
        .unwrap();
    assert_eq!(range.rows[0][0].value, CellValue::Number(42.0));
}

#[tokio::test]
async fn update_cells_roundtrips_legacy_xls() {
    if !soffice_available() {
        eprintln!("skipping update_cells_roundtrips_legacy_xls: soffice not installed");
        return;
    }
    let path = create_legacy_xls_fixture("xls_update_cells");
    let src = source(&path);

    let data = vec![vec!["X".to_string(), "Y".to_string()]];
    let result = update_cells(&src, None, "Sheet1", "A3", &data)
        .await
        .unwrap();
    assert_eq!(result.updated_count, 2);

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        &bytes[0..4],
        &OLE2_MAGIC,
        "output must remain a real OLE2 .xls file, not a renamed .xlsx"
    );

    let range = get_sheet_range(&src, "Sheet1", "A3:B3", false)
        .await
        .unwrap();
    assert_eq!(range.rows[0][0].value, CellValue::String("X".to_string()));
    assert_eq!(range.rows[0][1].value, CellValue::String("Y".to_string()));
}
