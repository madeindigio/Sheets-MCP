//! Integration tests for `read_template_metadata`.

use std::io::Write;
use std::path::PathBuf;

use sheets_mcp::file_source::FileSource;
use sheets_mcp::metadata::read_template_metadata;

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn test_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("sheets_mcp_template_metadata");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A workbook whose notes live in a drawing text box (as in the AEI travel
/// template) and in a cell comment, plus one merged range.
fn create_template(name: &str) -> PathBuf {
    let path = test_dir().join(format!("{name}.xlsx"));
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    let parts = [
        (
            "xl/workbook.xml".to_string(),
            format!(r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="{REL}"><sheets><sheet name="Viajes" sheetId="1" r:id="rId1"/></sheets></workbook>"#),
        ),
        (
            "xl/_rels/workbook.xml.rels".to_string(),
            format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#),
        ),
        (
            "xl/worksheets/sheet1.xml".to_string(),
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/><mergeCells count="1"><mergeCell ref="A5:A6"/></mergeCells></worksheet>"#.to_string(),
        ),
        (
            "xl/worksheets/_rels/sheet1.xml.rels".to_string(),
            format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/drawing" Target="../drawings/drawing1.xml"/><Relationship Id="rId2" Type="{REL}/comments" Target="/xl/comments1.xml"/></Relationships>"#),
        ),
        (
            "xl/drawings/drawing1.xml".to_string(),
            r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><xdr:twoCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>12</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:sp><xdr:txBody><a:p><a:r><a:t>(3) Tipo: Debe elegir entre</a:t></a:r><a:r><a:t> las opciones:</a:t></a:r></a:p><a:p/><a:p><a:r><a:t> · Opción &amp; A</a:t></a:r></a:p></xdr:txBody></xdr:sp></xdr:twoCellAnchor></xdr:wsDr>"#.to_string(),
        ),
        (
            "xl/comments1.xml".to_string(),
            r#"<comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><commentList><comment ref="B4"><text><t>Comentario plano</t></text></comment><comment ref="C4"><text><r><t>Nota </t></r><r><t>enriquecida</t></r></text></comment></commentList></comments>"#.to_string(),
        ),
    ];
    for (name, content) in parts {
        zip.start_file(name, options).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    path
}

fn source(path: &std::path::Path) -> FileSource {
    FileSource::Path {
        file_path: path.display().to_string(),
    }
}

#[tokio::test]
async fn reads_drawing_text_boxes_comments_and_merged_ranges() {
    let path = create_template("metadata_basic");
    let metadata = read_template_metadata(&source(&path), None).await.unwrap();
    let sheet = &metadata.sheets[0];
    assert_eq!(sheet.sheet_name, "Viajes");
    assert!(sheet.notes_available);
    assert_eq!(sheet.drawing_notes.len(), 1);
    assert_eq!(sheet.drawing_notes[0].anchor.as_deref(), Some("A13"));
    assert_eq!(
        sheet.drawing_notes[0].paragraphs,
        vec![
            "(3) Tipo: Debe elegir entre las opciones:",
            "",
            " · Opción & A"
        ]
    );
    let comments: Vec<(&str, &str)> = sheet
        .comments
        .iter()
        .map(|c| (c.cell.as_str(), c.text.as_str()))
        .collect();
    assert_eq!(
        comments,
        vec![("B4", "Comentario plano"), ("C4", "Nota enriquecida")]
    );
    assert_eq!(sheet.merged, vec!["A5:A6"]);
}

#[tokio::test]
async fn unknown_sheet_is_an_error() {
    let path = create_template("metadata_unknown_sheet");
    let error = read_template_metadata(&source(&path), Some("Otra"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Viajes"));
}
