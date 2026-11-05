//! Template metadata that cell reads do not show: the text of drawing text
//! boxes (column notes printed in a shape), cell comments and merged ranges.
//!
//! Notes are read from the OOXML parts directly (`xl/drawings/*.xml`,
//! `xl/comments*.xml`) so that every shape, grouped shapes included, and
//! plain-text comments are returned. Legacy `.xls` files report
//! `notes_available: false`.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::SheetsError;
use crate::file_source::FileSource;
use crate::types::{
    is_legacy_xls, SheetTemplateMetadata, TemplateComment, TemplateDrawingNote, TemplateMetadata,
};

type Archive = zip::ZipArchive<std::fs::File>;

/// Returns the notes and merged ranges of every sheet, or of `sheet_name`.
///
/// # Errors
/// - [`SheetsError::SheetNotFound`] if `sheet_name` doesn't exist
/// - [`SheetsError::Spreadsheet`] if the workbook can't be parsed
pub async fn read_template_metadata(
    source: &FileSource,
    sheet_name: Option<&str>,
) -> Result<TemplateMetadata, SheetsError> {
    let path = source.resolve().await?;
    let sheets = if is_legacy_xls(&path) {
        legacy_sheet_names(&path)?
            .into_iter()
            .map(|name| SheetTemplateMetadata {
                sheet_name: name,
                notes_available: false,
                drawing_notes: Vec::new(),
                comments: Vec::new(),
                merged: Vec::new(),
            })
            .collect()
    } else {
        read_xlsx_metadata(&path)?
    };
    let available: Vec<String> = sheets.iter().map(|s| s.sheet_name.clone()).collect();
    let selected: Vec<SheetTemplateMetadata> = sheets
        .into_iter()
        .filter(|sheet| sheet_name.is_none_or(|name| sheet.sheet_name == name))
        .collect();
    match sheet_name {
        Some(name) if selected.is_empty() => Err(SheetsError::sheet_not_found(name, &available)),
        _ => Ok(TemplateMetadata { sheets: selected }),
    }
}

fn legacy_sheet_names(path: &Path) -> Result<Vec<String>, SheetsError> {
    use calamine::Reader as _;
    let workbook =
        calamine::open_workbook_auto(path).map_err(|e| SheetsError::Spreadsheet(e.to_string()))?;
    Ok(workbook.sheet_names().to_vec())
}

fn read_xlsx_metadata(path: &Path) -> Result<Vec<SheetTemplateMetadata>, SheetsError> {
    let file = std::fs::File::open(path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| SheetsError::Spreadsheet(e.to_string()))?;
    let workbook_part = "xl/workbook.xml";
    let workbook_rels = relationships(&mut archive, workbook_part)?;
    let sheets = sheet_entries(&read_part(&mut archive, workbook_part)?.unwrap_or_default())?;
    sheets
        .into_iter()
        .map(|(name, relation_id)| {
            let sheet_part = workbook_rels
                .get(&relation_id)
                .map(|(_, target)| target.clone());
            let (drawing_notes, comments, merged) = match sheet_part {
                Some(part) => {
                    let (notes, comments) = sheet_notes(&mut archive, &part)?;
                    let sheet_xml = read_part(&mut archive, &part)?.unwrap_or_default();
                    (notes, comments, merged_ranges(&sheet_xml)?)
                }
                None => (Vec::new(), Vec::new(), Vec::new()),
            };
            Ok(SheetTemplateMetadata {
                merged,
                sheet_name: name,
                notes_available: true,
                drawing_notes,
                comments,
            })
        })
        .collect()
}

/// `<mergeCell ref="A5:A6"/>` entries of a sheet part.
fn merged_ranges(xml: &str) -> Result<Vec<String>, SheetsError> {
    let mut reader = Reader::from_str(xml);
    let mut ranges = Vec::new();
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Empty(e) | Event::Start(e) if e.local_name().as_ref() == b"mergeCell" => {
                ranges.extend(attribute(&e, b"ref"));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(ranges)
}

fn sheet_notes(
    archive: &mut Archive,
    sheet_part: &str,
) -> Result<(Vec<TemplateDrawingNote>, Vec<TemplateComment>), SheetsError> {
    let rels = relationships(archive, sheet_part)?;
    let mut drawing_notes = Vec::new();
    let mut comments = Vec::new();
    for (relation_type, target) in rels.values() {
        let Some(xml) = read_part(archive, target)? else {
            continue;
        };
        if relation_type.ends_with("/drawing") {
            drawing_notes.extend(drawing_text_boxes(&xml)?);
        } else if relation_type.ends_with("/comments") {
            comments.extend(comment_texts(&xml)?);
        }
    }
    drawing_notes.sort_by(|a, b| a.anchor.cmp(&b.anchor));
    Ok((drawing_notes, comments))
}

fn read_part(archive: &mut Archive, part: &str) -> Result<Option<String>, SheetsError> {
    let Ok(mut entry) = archive.by_name(part) else {
        return Ok(None);
    };
    let mut content = String::new();
    entry.read_to_string(&mut content)?;
    Ok(Some(content))
}

fn rels_part(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// Resolves a relationship target against the directory of `part`.
fn resolve_target(part: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_string();
    }
    let mut segments: Vec<&str> = part.split('/').collect();
    segments.pop();
    for segment in target.split('/') {
        match segment {
            ".." => {
                segments.pop();
            }
            "." | "" => {}
            other => segments.push(other),
        }
    }
    segments.join("/")
}

/// Relationship id → (type, resolved target part).
fn relationships(
    archive: &mut Archive,
    part: &str,
) -> Result<HashMap<String, (String, String)>, SheetsError> {
    let Some(xml) = read_part(archive, &rels_part(part))? else {
        return Ok(HashMap::new());
    };
    let mut reader = Reader::from_str(&xml);
    let mut relations = HashMap::new();
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Empty(e) | Event::Start(e) if e.local_name().as_ref() == b"Relationship" => {
                let attr = |key: &[u8]| attribute(&e, key);
                if let (Some(id), Some(kind), Some(target)) =
                    (attr(b"Id"), attr(b"Type"), attr(b"Target"))
                {
                    relations.insert(id, (kind, resolve_target(part, &target)));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(relations)
}

/// (sheet name, relationship id) in workbook order.
fn sheet_entries(xml: &str) -> Result<Vec<(String, String)>, SheetsError> {
    let mut reader = Reader::from_str(xml);
    let mut sheets = Vec::new();
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Empty(e) | Event::Start(e) if e.local_name().as_ref() == b"sheet" => {
                if let (Some(name), Some(id)) = (attribute(&e, b"name"), attribute(&e, b"id")) {
                    sheets.push((name, id));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(sheets)
}

/// One note per text box (`txBody`), with its paragraphs and the top-left
/// cell of its anchor.
fn drawing_text_boxes(xml: &str) -> Result<Vec<TemplateDrawingNote>, SheetsError> {
    let mut reader = Reader::from_str(xml);
    let mut notes = Vec::new();
    let mut anchor: Option<String> = None;
    let (mut in_from, mut col, mut row) = (false, None::<u32>, None::<u32>);
    let mut field: Option<&'static str> = None;
    let mut paragraphs: Option<Vec<String>> = None;
    let mut in_text = false;
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Start(e) => match e.local_name().as_ref() {
                b"from" => in_from = true,
                b"col" if in_from => field = Some("col"),
                b"row" if in_from => field = Some("row"),
                b"txBody" => paragraphs = Some(Vec::new()),
                b"p" => {
                    if let Some(list) = paragraphs.as_mut() {
                        list.push(String::new());
                    }
                }
                b"t" => in_text = paragraphs.is_some(),
                _ => {}
            },
            Event::Empty(e) if e.local_name().as_ref() == b"p" => {
                if let Some(list) = paragraphs.as_mut() {
                    list.push(String::new());
                }
            }
            Event::Text(text) => {
                let value = text.unescape().map_err(xml_error)?;
                match field {
                    Some("col") => col = value.trim().parse().ok(),
                    Some("row") => row = value.trim().parse().ok(),
                    _ if in_text => {
                        if let Some(last) = paragraphs.as_mut().and_then(|list| list.last_mut()) {
                            last.push_str(&value);
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"col" | b"row" => field = None,
                b"from" => {
                    in_from = false;
                    anchor = col.zip(row).map(|(c, r)| cell_reference(c, r));
                }
                b"t" => in_text = false,
                b"twoCellAnchor" | b"oneCellAnchor" | b"absoluteAnchor" => anchor = None,
                b"txBody" => {
                    if let Some(list) = paragraphs.take() {
                        if list.iter().any(|p| !p.trim().is_empty()) {
                            notes.push(TemplateDrawingNote {
                                anchor: anchor.clone(),
                                paragraphs: list,
                            });
                        }
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(notes)
}

fn comment_texts(xml: &str) -> Result<Vec<TemplateComment>, SheetsError> {
    let mut reader = Reader::from_str(xml);
    let mut comments = Vec::new();
    let mut current: Option<TemplateComment> = None;
    let mut in_text = false;
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Start(e) => match e.local_name().as_ref() {
                b"comment" => {
                    current = Some(TemplateComment {
                        cell: attribute(&e, b"ref").unwrap_or_default(),
                        text: String::new(),
                    })
                }
                b"t" => in_text = current.is_some(),
                _ => {}
            },
            Event::Text(text) if in_text => {
                if let Some(comment) = current.as_mut() {
                    comment.text.push_str(&text.unescape().map_err(xml_error)?);
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"t" => in_text = false,
                b"comment" => {
                    if let Some(mut comment) = current.take() {
                        comment.text = comment.text.trim().to_string();
                        comments.push(comment);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(comments)
}

/// Zero-based drawing marker → A1 reference.
fn cell_reference(col: u32, row: u32) -> String {
    let mut letters = String::new();
    let mut value = col + 1;
    while value > 0 {
        let remainder = (value - 1) % 26;
        letters.insert(0, char::from(b'A' + remainder as u8));
        value = (value - 1) / 26;
    }
    format!("{letters}{}", row + 1)
}

fn attribute(element: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attr| attr.key.local_name().as_ref() == key)
        .and_then(|attr| attr.unescape_value().ok().map(|value| value.into_owned()))
}

fn xml_error(error: impl std::fmt::Display) -> SheetsError {
    SheetsError::Spreadsheet(format!("invalid template XML: {error}"))
}
