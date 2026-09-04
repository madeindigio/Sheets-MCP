//! MCP tool definitions and server wiring.
//!
//! This module defines the rmcp tool handlers, their input argument structs,
//! and the [`SheetsServer`] struct that registers everything.
//!
//! # Tools
//!
//! | Tool | Args Struct |
//! |------|-------------|
//! | `read_structure` | [`ReadStructureArgs`] |
//! | `count_sheet_rows` | [`CountSheetRowsArgs`] |
//! | `get_sheet_range` | [`GetSheetRangeArgs`] |
//! | `update_cell` | [`UpdateCellArgs`] |
//! | `update_cells` | [`UpdateCellsArgs`] |

use rmcp::{tool, tool_handler, tool_router};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::{ErrorData as McpError, ServerHandler, model::CallToolResult};
use rmcp::model::Content;
use rmcp::schemars::JsonSchema;
use serde::Deserialize;

use crate::file_source::FileSource;
use crate::reader;
use crate::types::CellRange;
use crate::writer;

// ──────────────────────────────────────────────
// Server struct with tool router
// ──────────────────────────────────────────────

/// The MCP server struct that handles all tool requests.
///
/// Created via [`SheetsServer::new`] and served over stdio in `main.rs`.
/// Implements [`ServerHandler`] via the rmcp `#[tool_handler]` macro.
#[derive(Debug, Clone)]
pub struct SheetsServer {
    tool_router: ToolRouter<SheetsServer>,
}

impl SheetsServer {
    /// Create a new server with all tools registered.
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for SheetsServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler(router = self.tool_router, name = "sheetsMCP")]
impl ServerHandler for SheetsServer {}

// ──────────────────────────────────────────────
// Tool argument structs
// ──────────────────────────────────────────────

/// Arguments for the `read_structure` tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReadStructureArgs {
    /// File source: either `file_path` or `url`.
    #[serde(flatten)]
    pub source: FileSource,
    /// Number of preview rows per sheet (default: 10).
    #[serde(default = "default_first_n_rows")]
    #[schemars(schema_with = "first_n_rows_schema")]
    pub first_n_rows: u32,
}

fn default_first_n_rows() -> u32 {
    10
}

fn first_n_rows_schema(
    _: &mut rmcp::schemars::SchemaGenerator,
) -> rmcp::schemars::Schema {
    // Emit a plain non-negative integer schema to avoid client warnings
    // about unknown integer formats like "uint32".
    rmcp::schemars::json_schema!({
        "type": "integer",
        "minimum": 0
    })
}

/// Arguments for the `count_sheet_rows` tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CountSheetRowsArgs {
    /// File source: either `file_path` or `url`.
    #[serde(flatten)]
    pub source: FileSource,
    /// Name of the sheet to query.
    pub sheet_name: String,
}

/// Arguments for the `get_sheet_range` tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetSheetRangeArgs {
    /// File source: either `file_path` or `url`.
    #[serde(flatten)]
    pub source: FileSource,
    /// Name of the sheet.
    pub sheet_name: String,
    /// Range string (e.g. `"A1:D100"`).
    pub range: String,
    /// If `true`, include cell formatting (slower).
    #[serde(default)]
    pub include_format: bool,
}

/// Arguments for the `update_cell` tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UpdateCellArgs {
    /// File source: either `file_path` or `url`.
    #[serde(flatten)]
    pub source: FileSource,
    /// Name of the sheet.
    pub sheet_name: String,
    /// Cell coordinate (e.g. `"B5"`).
    pub coordinate: String,
    /// New value as a string.
    pub value: String,
    /// Value type: `"number"`, `"bool"`, or `"string"`.
    pub value_type: String,
    /// Output path (required when source is a URL).
    pub output_path: Option<String>,
}

/// Arguments for the `update_cells` tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UpdateCellsArgs {
    /// File source: either `file_path` or `url`.
    #[serde(flatten)]
    pub source: FileSource,
    /// Name of the sheet.
    pub sheet_name: String,
    /// Top-left destination cell (e.g. `"B2"`).
    pub destination: String,
    /// 2D array of string values (row-major).
    pub data: Vec<Vec<String>>,
    /// Output path (required when source is a URL).
    pub output_path: Option<String>,
}

// ──────────────────────────────────────────────
// Tool implementations
// ──────────────────────────────────────────────

#[tool_router(router = tool_router)]
impl SheetsServer {
    #[tool(description = "Returns the structure of a workbook: sheet names, dimensions, and the first N rows with values and formatting (colors, fonts, borders). Use this to understand the spreadsheet layout and identify cells by appearance (e.g., 'the gray cells'). Accepts a file_path or url.")]
    async fn read_structure(
        &self,
        Parameters(args): Parameters<ReadStructureArgs>,
    ) -> Result<CallToolResult, McpError> {
        let structure = reader::read_structure(&args.source, args.first_n_rows)
            .await
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&structure)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Returns the total number of rows and columns in a specific sheet. Accepts a file_path or url.")]
    async fn count_sheet_rows(
        &self,
        Parameters(args): Parameters<CountSheetRowsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let count = reader::count_sheet_rows(&args.source, &args.sheet_name)
            .await
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&count)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Returns cell values from a given range (e.g., 'A1:D100'). Set include_format=true to also get colors, fonts, and borders for each cell. Accepts a file_path or url.")]
    async fn get_sheet_range(
        &self,
        Parameters(args): Parameters<GetSheetRangeArgs>,
    ) -> Result<CallToolResult, McpError> {
        let _range: CellRange = args.range.parse::<CellRange>()
            .map_err(|e: crate::error::SheetsError| McpError::invalid_params(e.to_string(), None))?;
        let data = reader::get_sheet_range(
            &args.source,
            &args.sheet_name,
            &args.range,
            args.include_format,
        )
        .await
        .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&data)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Updates a single cell's value. Preserves all existing formulas and formatting. Accepts a file_path or url. If using a URL, provide output_path to save the modified file.")]
    async fn update_cell(
        &self,
        Parameters(args): Parameters<UpdateCellArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = writer::update_cell(
            &args.source,
            args.output_path.as_deref(),
            &args.sheet_name,
            &args.coordinate,
            &args.value,
            &args.value_type,
        )
        .await
        .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Pastes a 2D array of values starting at a destination cell. Preserves all existing formulas and formatting. Accepts a file_path or url. If using a URL, provide output_path to save the modified file.")]
    async fn update_cells(
        &self,
        Parameters(args): Parameters<UpdateCellsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let result = writer::update_cells(
            &args.source,
            args.output_path.as_deref(),
            &args.sheet_name,
            &args.destination,
            &args.data,
        )
        .await
        .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}
