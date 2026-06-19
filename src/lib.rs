//! # sheets_mcp
//!
//! A high-performance MCP server for reading and writing Excel spreadsheets.
//!
//! ## Architecture
//!
//! ```text
//! LLM Client (stdio) → rmcp server → Tool handlers → reader (calamine/umya) / writer (umya)
//! ```
//!
//! Two read paths:
//! - **Fast path (calamine)**: Values only, ~1M cells/sec.
//! - **Format path (umya-spreadsheet)**: Values + formatting.
//!
//! One write path:
//! - **umya-spreadsheet**: Read-modify-write for `.xlsx`. Preserves formulas and formatting.
//!
//! ## MCP Tools (5)
//!
//! | Tool | Description |
//! |------|-------------|
//! | `read_structure` | Workbook structure with preview rows |
//! | `count_sheet_rows` | Fast row/column count |
//! | `get_sheet_range` | Cell values from a range |
//! | `update_cell` | Update a single cell value |
//! | `update_cells` | Paste a 2D array of values |

pub mod error;
pub mod file_source;
pub mod reader;
pub mod types;
pub mod tools;
pub mod writer;
