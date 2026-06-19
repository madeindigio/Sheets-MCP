//! File input resolution for the MCP server.
//!
//! Every tool accepts either a local `file_path` or a remote `url`. This module
//! handles resolving either form to a local path on disk.
//!
//! # URL Download & Caching
//!
//! URLs are downloaded via `reqwest` and cached in `$TMPDIR/sheets_mcp/`.
//! The cache key is a hash of the URL, preserving the file extension for
//! format detection. There is no TTL — delete the cache directory manually
//! to force re-download.
//!
//! # Write Targets
//!
//! - Local `file_path` → writes back in-place (or to `output_path` if provided)
//! - Remote `url` → requires `output_path` to save modified file

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::SheetsError;

/// Either a local file path or a remote URL.
///
/// Used as input for every MCP tool. The two variants are mutually exclusive —
/// provide one or the other, never both.
///
/// # Examples
///
/// ```json
/// { "file_path": "/home/user/data.xlsx" }
/// { "url": "https://cdn.example.com/data.xlsx" }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FileSource {
    /// A remote URL to download.
    Url { url: String },
    /// A local file path.
    Path { file_path: String },
}

impl FileSource {
    /// Resolve to a local path on disk.
    /// - `Path`: returns the path directly.
    /// - `Url`: downloads (with caching) and returns the cached path.
    pub async fn resolve(&self) -> Result<PathBuf, SheetsError> {
        match self {
            FileSource::Path { file_path } => {
                let path = PathBuf::from(file_path);
                if !path.exists() {
                    return Err(SheetsError::FileNotFound(
                        file_path.clone(),
                    ));
                }
                Ok(path)
            }
            FileSource::Url { url } => download(url).await,
        }
    }

    /// Determine the write target path.
    /// - `Path` with no `output_path`: write back to the same file.
    /// - `Path` with `output_path`: write to `output_path`.
    /// - `Url` with `output_path`: write to `output_path`.
    /// - `Url` without `output_path`: error.
    pub fn write_target(
        &self,
        output_path: Option<&str>,
    ) -> Result<PathBuf, SheetsError> {
        match self {
            FileSource::Path { file_path } => {
                Ok(PathBuf::from(
                    output_path.unwrap_or(file_path),
                ))
            }
            FileSource::Url { .. } => match output_path {
                Some(p) => Ok(PathBuf::from(p)),
                None => Err(SheetsError::MissingOutputPath),
            },
        }
    }
}

// ── URL download + cache ─────────────────────

/// Cache directory: `$TMPDIR/sheets_mcp/`.
fn cache_dir() -> PathBuf {
    std::env::temp_dir().join("sheets_mcp")
}

/// Cache path based on URL hash, preserving the file extension.
fn cache_path(url: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    let hash = hasher.finish();

    let ext = std::path::Path::new(url)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("xlsx");

    cache_dir().join(format!("{:x}.{}", hash, ext))
}

/// Download a URL and cache it locally.
/// Returns the local cached path.
async fn download(url: &str) -> Result<PathBuf, SheetsError> {
    let path = cache_path(url);

    // If already cached, skip download.
    if path.exists() {
        tracing::info!("Using cached: {} → {:?}", url, path);
        return Ok(path);
    }

    // Ensure cache dir exists.
    std::fs::create_dir_all(cache_dir())?;

    // Download.
    let response = reqwest::get(url)
        .await?
        .error_for_status()?;
    let bytes = response.bytes().await?;
    std::fs::write(&path, &bytes)?;

    tracing::info!(
        "Downloaded {} → {:?} ({} bytes)",
        url,
        path,
        bytes.len()
    );
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_source_resolves() {
        let src = FileSource::Path {
            file_path: "/tmp/test.xlsx".into(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        // Path doesn't exist → should error.
        let result = rt.block_on(src.resolve());
        assert!(result.is_err());
    }

    #[test]
    fn write_target_path_no_output() {
        let src = FileSource::Path {
            file_path: "/tmp/test.xlsx".into(),
        };
        let target = src.write_target(None).unwrap();
        assert_eq!(target, PathBuf::from("/tmp/test.xlsx"));
    }

    #[test]
    fn write_target_path_with_output() {
        let src = FileSource::Path {
            file_path: "/tmp/test.xlsx".into(),
        };
        let target =
            src.write_target(Some("/tmp/out.xlsx")).unwrap();
        assert_eq!(target, PathBuf::from("/tmp/out.xlsx"));
    }

    #[test]
    fn write_target_url_with_output() {
        let src = FileSource::Url {
            url: "https://example.com/data.xlsx".into(),
        };
        let target =
            src.write_target(Some("/tmp/out.xlsx")).unwrap();
        assert_eq!(target, PathBuf::from("/tmp/out.xlsx"));
    }

    #[test]
    fn write_target_url_no_output_errors() {
        let src = FileSource::Url {
            url: "https://example.com/data.xlsx".into(),
        };
        let result = src.write_target(None);
        assert!(matches!(result, Err(SheetsError::MissingOutputPath)));
    }
}
