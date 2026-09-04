//! Unified error types for sheets_mcp operations.
//!
//! All internal errors are converted to [`SheetsError`], which is then
//! converted to a string at the MCP tool boundary.

use std::path::PathBuf;

/// Unified error type for sheets_mcp operations.
///
/// Every fallible operation returns `Result<T, SheetsError>`. At the MCP
/// tool boundary, errors are converted to strings for the JSON-RPC response.
#[derive(Debug, thiserror::Error)]
pub enum SheetsError {
    /// The specified file does not exist on disk.
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// The specified sheet name was not found in the workbook.
    #[error("Sheet not found: '{sheet}' (available sheets: {available})")]
    SheetNotFound {
        /// The requested sheet name.
        sheet: String,
        /// Comma-separated list of available sheet names.
        available: String,
    },

    /// A cell coordinate string could not be parsed.
    #[error("Invalid cell coordinate: {0}")]
    InvalidCoordinate(String),

    /// An I/O error occurred (file read/write, directory creation).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// An HTTP error occurred during URL download.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// An error from calamine or umya-spreadsheet.
    #[error("Spreadsheet error: {0}")]
    Spreadsheet(String),

    /// Write tools require `output_path` when the source is a URL.
    #[error("Output path required when source is a URL")]
    MissingOutputPath,

    /// The write target path is an existing directory, not a file.
    #[error("Write target is a directory: {0}")]
    WriteTargetIsDirectory(PathBuf),

    /// The file format is not supported (e.g. `.xls` for writes).
    #[error("Unsupported file format: {0}")]
    UnsupportedFormat(String),

    /// A required field is missing from the tool input.
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Shelling out to an external tool (e.g. LibreOffice) for format
    /// conversion failed, timed out, or the binary was not found.
    #[error("External conversion failed: {0}")]
    ExternalConversionFailed(String),
}

impl SheetsError {
    /// Build a SheetNotFound error with the list of available sheet names.
    pub fn sheet_not_found(name: &str, available: &[String]) -> Self {
        let list = if available.is_empty() {
            "(workbook has no sheets)".to_string()
        } else {
            available.join(", ")
        };
        SheetsError::SheetNotFound {
            sheet: name.to_string(),
            available: list,
        }
    }
}

/// Helper to convert a calamine error into SheetsError.
impl From<calamine::Error> for SheetsError {
    fn from(err: calamine::Error) -> Self {
        SheetsError::Spreadsheet(err.to_string())
    }
}
