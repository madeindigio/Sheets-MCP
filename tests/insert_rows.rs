//! Integration tests for `insert_rows`.

use sheets_mcp::file_source::FileSource;
use sheets_mcp::rows::insert_rows;
use std::path::PathBuf;
use umya_spreadsheet::new_file;

fn test_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("sheets_mcp_insert_rows");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Header in row 1, two pre-formatted data rows (2-3) with a yellow fill,
/// a custom height and formulas, a note in row 5 and a merged range below.
fn create_template(name: &str) -> PathBuf {
    let mut book = new_file();
    let ws = book.get_sheet_mut(&0).unwrap();
    ws.get_cell_mut("A1").set_value("Nights");
    ws.get_cell_mut("B1").set_value("Price");
    ws.get_cell_mut("C1").set_value("Total");
    for row in 2..=3u32 {
        for column in 1..=3u32 {
            ws.get_cell_mut((column, row))
                .get_style_mut()
                .get_fill_mut()
                .get_pattern_fill_mut()
                .get_background_color_mut()
                .set_argb("FFFFFF00");
        }
        ws.get_cell_mut((3, row))
            .set_formula(format!("A{row}*B{row}"));
        ws.get_row_dimension_mut(&row)
            .set_height(30.0)
            .set_custom_height(true);
    }
    ws.get_cell_mut("A5").set_value("(1) Note below the table");
    ws.get_cell_mut("C6").set_formula("SUM(C2:C3)");
    ws.add_merge_cells("A7:C7");
    let path = test_dir().join(format!("{name}.xlsx"));
    umya_spreadsheet::writer::xlsx::write(&book, &path).unwrap();
    path
}

fn source(path: &std::path::Path) -> FileSource {
    FileSource::Path {
        file_path: path.display().to_string(),
    }
}

#[tokio::test]
async fn inserts_rows_with_template_format_and_shifted_formulas() {
    let path = create_template("insert_basic");
    let result = insert_rows(&source(&path), None, "Sheet1", 3, 3, None)
        .await
        .unwrap();
    assert_eq!((result.first_new_row, result.last_new_row), (4, 6));
    assert_eq!(result.formulas_copied, 3);

    let book = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
    let ws = book.get_sheet_by_name("Sheet1").unwrap();
    for row in 4..=6u32 {
        let total = ws.get_cell((3, row)).unwrap();
        assert_eq!(total.get_formula(), format!("A{row}*B{row}"));
        let fill = ws
            .get_cell((1, row))
            .unwrap()
            .get_style()
            .get_fill()
            .unwrap()
            .get_pattern_fill()
            .unwrap()
            .get_background_color()
            .unwrap()
            .get_argb()
            .to_string();
        assert_eq!(fill, "FFFFFF00");
        assert_eq!(*ws.get_row_dimension(&row).unwrap().get_height(), 30.0);
    }
    // The template rows keep their own formulas.
    assert_eq!(ws.get_cell("C3").unwrap().get_formula(), "A3*B3");
    // Content below moved down by three rows.
    assert_eq!(ws.get_value("A8"), "(1) Note below the table");
    assert_eq!(ws.get_value("A5"), "");
    assert!(ws
        .get_merge_cells()
        .iter()
        .any(|range| range.get_range() == "A10:C10"));
}

#[tokio::test]
async fn rejects_a_template_row_below_the_insertion_point() {
    let path = create_template("insert_invalid");
    let error = insert_rows(&source(&path), None, "Sheet1", 2, 1, Some(3))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("copy_from_row"));
}

/// C2:C4 share one formula (`A2*B2`), stored once on C2 as Excel does.
fn create_shared_formula_workbook(name: &str) -> PathBuf {
    use umya_spreadsheet::structs::{CellFormula, CellFormulaValues};
    let mut book = new_file();
    let ws = book.get_sheet_mut(&0).unwrap();
    for row in 2..=4u32 {
        ws.get_cell_mut((1, row)).set_value_number(f64::from(row));
        ws.get_cell_mut((2, row)).set_value_number(10.0);
        let mut formula = CellFormula::default();
        formula.set_formula_type(CellFormulaValues::Shared);
        formula.set_shared_index(0);
        if row == 2 {
            formula.set_reference("C2:C4");
            formula.set_text("A2*B2");
        }
        ws.get_cell_mut((3, row))
            .get_cell_value_mut()
            .set_formula_obj(formula);
    }
    let path = test_dir().join(format!("{name}.xlsx"));
    umya_spreadsheet::writer::xlsx::write(&book, &path).unwrap();
    path
}

#[tokio::test]
async fn overwriting_a_shared_formula_keeps_the_other_cells_formulas() {
    use sheets_mcp::types::CellInput;
    let path = create_shared_formula_workbook("shared_overwrite");
    let before = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
    assert_eq!(
        before
            .get_sheet(&0)
            .unwrap()
            .get_cell("C3")
            .unwrap()
            .get_formula(),
        "A3*B3"
    );

    let data: Vec<Vec<CellInput>> = serde_json::from_str("[[42]]").unwrap();
    sheets_mcp::writer::update_cells(&source(&path), None, "Sheet1", "C2", &data)
        .await
        .unwrap();
    insert_rows(&source(&path), None, "Sheet1", 4, 1, None)
        .await
        .unwrap();

    let book = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
    let ws = book.get_sheet(&0).unwrap();
    assert!(!ws.get_cell("C2").unwrap().is_formula());
    assert_eq!(ws.get_cell("C3").unwrap().get_formula(), "A3*B3");
    assert_eq!(ws.get_cell("C4").unwrap().get_formula(), "A4*B4");
    assert_eq!(ws.get_cell("C5").unwrap().get_formula(), "A5*B5");
    // No stale cached result: spreadsheet programs recompute on open.
    assert_eq!(ws.get_cell("C3").unwrap().get_value(), "");
}
