# Phase 4: MCP Server Wiring

## Goal
Wire all 5 tools into `rmcp` MCP tool handlers, set up the stdio transport, and build the main entry point.

## Implementation Plan

### `src/tools.rs`

Define MCP tools using `rmcp`'s `#[tool]` macro.

### 5 MCP Tools

| # | Tool Name | Description |
|---|-----------|-------------|
| 1 | `read_structure` | Returns sheet names, column count, row count, and the first N rows with values AND formatting (colors, fonts, borders) |
| 2 | `count_sheet_rows` | Returns the total number of rows and columns in a sheet |
| 3 | `get_sheet_range` | Returns cell values (and optionally formatting) from a given range like `A1:D100` |
| 4 | `update_cell` | Updates a single cell's value. Preserves formulas and formatting. |
| 5 | `update_cells` | Pastes a 2D array of values starting at a destination cell. Preserves formulas and formatting. |

### Tool Handler Pattern

Each tool is a function annotated with `rmcp`'s tool macro:

```rust
use rmcp::tool;

#[derive(Debug, serde::Deserialize)]
pub struct ReadStructureArgs {
    #[serde(flatten)]
    pub source: FileSource,
    #[serde(default = "default_first_n_rows")]
    pub first_n_rows: u32,
}

fn default_first_n_rows() -> u32 { 10 }

#[tool(description = "Returns the structure of a workbook: sheet names, dimensions, and the first N rows with values and formatting (colors, fonts, borders). Use this to understand the spreadsheet layout and identify cells by appearance (e.g., 'the gray cells'). Accepts a file_path or url.")]
pub async fn read_structure(args: ReadStructureArgs) -> Result<String, String> {
    let structure = reader::read_structure(&args.source, args.first_n_rows).await
        .map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&structure)
        .map_err(|e| e.to_string())
}

#[derive(Debug, serde::Deserialize)]
pub struct CountSheetRowsArgs {
    #[serde(flatten)]
    pub source: FileSource,
    pub sheet_name: String,
}

#[tool(description = "Returns the total number of rows and columns in a specific sheet. Accepts a file_path or url.")]
pub async fn count_sheet_rows(args: CountSheetRowsArgs) -> Result<String, String> {
    let count = reader::count_sheet_rows(&args.source, &args.sheet_name).await
        .map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&count)
        .map_err(|e| e.to_string())
}

#[derive(Debug, serde::Deserialize)]
pub struct GetSheetRangeArgs {
    #[serde(flatten)]
    pub source: FileSource,
    pub sheet_name: String,
    pub range: String,               // e.g. "A1:D100"
    #[serde(default)]
    pub include_format: bool,        // default false (fast path)
}

#[tool(description = "Returns cell values from a given range (e.g., 'A1:D100'). Set include_format=true to also get colors, fonts, and borders for each cell. Accepts a file_path or url.")]
pub async fn get_sheet_range(args: GetSheetRangeArgs) -> Result<String, String> {
    let range = args.range.parse::<CellRange>().map_err(|e| e.to_string())?;
    let data = reader::get_sheet_range(&args.source, &args.sheet_name, &range, args.include_format).await
        .map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&data)
        .map_err(|e| e.to_string())
}

#[derive(Debug, serde::Deserialize)]
pub struct UpdateCellArgs {
    #[serde(flatten)]
    pub source: FileSource,
    pub sheet_name: String,
    pub coordinate: String,          // e.g. "B5"
    pub value: String,               // "42", "hello", "true"
    pub value_type: String,          // "String", "Number", "Bool"
    pub output_path: Option<String>, // required if source is a URL
}

#[tool(description = "Updates a single cell's value. Preserves all existing formulas and formatting. Accepts a file_path or url. If using a URL, provide output_path to save the modified file.")]
pub async fn update_cell(args: UpdateCellArgs) -> Result<String, String> {
    let value_type = parse_value_type(&args.value_type)?;
    let update = CellUpdate { coordinate: args.coordinate, value: args.value, value_type };
    let result = writer::update_cell(&args.source, args.output_path.as_deref(), &args.sheet_name, update).await
        .map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&result)
        .map_err(|e| e.to_string())
}

#[derive(Debug, serde::Deserialize)]
pub struct UpdateCellsArgs {
    #[serde(flatten)]
    pub source: FileSource,
    pub sheet_name: String,
    pub destination: String,         // top-left cell e.g. "B2"
    pub data: Vec<Vec<String>>,      // 2D array
    pub output_path: Option<String>, // required if source is a URL
}

#[tool(description = "Pastes a 2D array of values starting at a destination cell. Preserves all existing formulas and formatting. Accepts a file_path or url. If using a URL, provide output_path to save the modified file.")]
pub async fn update_cells(args: UpdateCellsArgs) -> Result<String, String> {
    let result = writer::update_cells(&args.source, args.output_path.as_deref(), &args.sheet_name, &args.destination, args.data).await
        .map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&result)
        .map_err(|e| e.to_string())
}
```

### `src/main.rs`

```rust
use rmcp::transport::stream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing — logs go to stderr (stdio transport uses stdout)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    // Create and serve the MCP server
    let server = rmcp::server::serve(
        tools::create_server(),
    )
    .transport(stream::stdio())
    .serve()
    .await?;

    Ok(())
}
```

### `src/lib.rs`

```rust
pub mod error;
pub mod types;
pub mod file_source;
pub mod reader;
pub mod writer;
pub mod tools;
```

## Acceptance Criteria
- [ ] Server starts and advertises 5 tools via MCP `tools/list`
- [ ] Each tool callable via MCP `tools/call`
- [ ] Error messages propagate cleanly as MCP error responses
- [ ] Logging goes to stderr only (no stdio interference)
- [ ] Works with VS Code, Claude Desktop, or any MCP client