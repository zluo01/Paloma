use std::path::Path;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::entity::{Capability, CapabilityMeta, Tool, ToolResult};

// Based on ForgeCode's fs_read tool shape and LLM-facing behavior:
// https://github.com/tailcallhq/forgecode/blob/main/crates/forge_services/src/tool_services/fs_read.rs
// https://github.com/tailcallhq/forgecode/blob/main/crates/forge_domain/src/tools/descriptions/fs_read.md
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MiB
const MAX_READ_LINES: u64 = 2_000;
const MAX_LINE_CHARS: usize = 2_000;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileSystemReadArgs {
    /// Absolute path to the file to read.
    pub path: String,
    /// 1-based first line of the range to return.
    #[serde(default)]
    pub start_line: Option<u64>,
    /// Inclusive 1-based last line of the range to return.
    #[serde(default)]
    pub end_line: Option<u64>,
}

pub struct FileSystemRead;

impl Capability for FileSystemRead {
    fn id(&self) -> &'static str {
        "fs_read"
    }

    fn metadata(&self) -> CapabilityMeta {
        CapabilityMeta {
            name: "Read File".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "Read a file's contents from an absolute path.".to_string(),
            icon: None,
            homepage: None,
            author: None,
        }
    }
}

#[async_trait::async_trait]
impl Tool for FileSystemRead {
    type Args = FileSystemReadArgs;

    const NAME: &'static str = "fs_read";
    const DESCRIPTION: &'static str = concat!(
        "Reads a file from the local filesystem. Use this when you need to inspect code, ",
        "configuration, documentation, logs, text data, images, or PDFs. The path parameter must ",
        "be an absolute path, not a relative path. If the user provides a path to a file, assume ",
        "that path is valid; it is okay to call this tool for a missing file because an error will ",
        "be returned. Files larger than 10 MiB return an error. By default, this reads up to ",
        "2,000 lines starting from the beginning of the file. You can optionally specify ",
        "start_line and end_line to read a specific range, especially for long files, but prefer ",
        "reading the whole file by omitting them unless a narrower range is clearly useful. Text ",
        "results are returned in rg \"\" -n format, with line numbers starting at 1. Lines longer ",
        "than 2,000 characters are truncated. Images such as PNG, JPG, GIF, and WebP are returned ",
        "as binary content with a MIME type for downstream visual handling. PDFs are also returned ",
        "as binary content with MIME type application/pdf. Jupyter notebooks (.ipynb files) are ",
        "read as plain JSON text so the cell structure, outputs, and embedded content can be ",
        "inspected. This tool can only read files, not directories."
    );

    async fn invoke(&self, args: Self::Args) -> Result<ToolResult, String> {
        let path = Path::new(&args.path);
        assert_absolute_path(path)?;

        assert_file_size(path, MAX_FILE_SIZE).await?;

        // Read file content to detect MIME type
        let raw_content = tokio::fs::read(path)
            .await
            .map_err(|e| format!("Failed to read file from {}: {e}", path.display()))?;

        // Detect MIME type
        let mime_type = detect_mime_type(path, &raw_content);

        // Handle visual content (PDFs and images)
        if is_visual_content(&mime_type) {
            return Ok(ToolResult::Binary {
                mime_type,
                data: raw_content,
            });
        }

        // Handle text content (including Jupyter notebooks)
        let (start_line, end_line) = resolve_range(args.start_line, args.end_line, MAX_READ_LINES);

        // Convert bytes to UTF-8 string
        let full_content = String::from_utf8(raw_content)
            .map_err(|e| format!("Failed to read file as UTF-8 from {}: {e}", path.display()))?;

        // Now extract the requested range from the content we already have
        let lines: Vec<&str> = full_content.lines().collect();
        let total_lines = lines.len() as u64;

        // Convert to 0-based indexing and clamp to valid range
        let start_pos = start_line
            .saturating_sub(1)
            .min(total_lines.saturating_sub(1));
        let end_pos = end_line
            .saturating_sub(1)
            .min(total_lines.saturating_sub(1));

        let (first_line, selected_lines): (u64, &[&str]) = if total_lines == 0 {
            (1, &[])
        } else if start_pos == 0 && end_pos >= total_lines.saturating_sub(1) {
            (1, &lines)
        } else {
            (
                start_pos + 1,
                lines
                    .get(start_pos as usize..=end_pos as usize)
                    .unwrap_or(&[]),
            )
        };

        Ok(ToolResult::Text(format_numbered_lines(
            selected_lines,
            first_line,
            MAX_LINE_CHARS,
        )))
    }
}

fn assert_absolute_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("Path must be absolute, got: {}", path.display()));
    }
    Ok(())
}

/// Validates that file size does not exceed the maximum allowed file size.
async fn assert_file_size(path: &Path, max_file_size: u64) -> Result<(), String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|e| format!("Failed to read metadata for {}: {e}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("Path is not a file: {}", path.display()));
    }

    let file_size = metadata.len();
    if file_size > max_file_size {
        return Err(format!(
            "File size ({file_size} bytes) exceeds the maximum allowed size of {max_file_size} bytes"
        ));
    }
    Ok(())
}

/// Resolves an optional (start, end) line pair into a concrete range, capped
/// to `max_lines` total lines.
fn resolve_range(start: Option<u64>, end: Option<u64>, max_lines: u64) -> (u64, u64) {
    let max_span = max_lines.saturating_sub(1);
    match (start, end) {
        (Some(s), Some(e)) => {
            let s = s.max(1);
            let e = e.max(s).min(s.saturating_add(max_span));
            (s, e)
        }
        (Some(s), None) => {
            let s = s.max(1);
            (s, s.saturating_add(max_span))
        }
        (None, Some(e)) => (1, e.min(max_lines).max(1)),
        (None, None) => (1, max_lines),
    }
}

/// Truncates a line to the maximum length if it exceeds the limit.
fn truncate_line(line: &str, max_length: usize) -> String {
    if line.len() > max_length {
        // Use char indices to avoid panicking on unicode boundaries
        let truncated = line
            .char_indices()
            .take_while(|(idx, _)| *idx < max_length)
            .map(|(_, ch)| ch)
            .collect::<String>();
        format!("{truncated}... [truncated, line exceeds {max_length} chars]")
    } else {
        line.to_string()
    }
}

fn format_numbered_lines(lines: &[&str], first_line: u64, max_line_chars: usize) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            format!(
                "{}:{}",
                first_line + idx as u64,
                truncate_line(line, max_line_chars)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Detects the MIME type of a file based on extension and content.
fn detect_mime_type(path: &Path, content: &[u8]) -> String {
    // Try infer crate first (checks magic numbers)
    if let Some(file_type) = infer::get(content) {
        return file_type.mime_type().to_string();
    }

    // Fallback to extension-based detection
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| match ext.to_lowercase().as_str() {
            "txt" | "md" | "rs" | "toml" | "yaml" | "yml" | "json" | "js" | "ts" | "py" | "sh" => {
                "text/plain"
            }
            "ipynb" => "application/json",
            "pdf" => "application/pdf",
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            _ => "text/plain",
        })
        .unwrap_or("text/plain")
        .to_string()
}

/// Checks if a MIME type represents visual content (images or PDFs).
fn is_visual_content(mime_type: &str) -> bool {
    mime_type.starts_with("image/") || mime_type == "application/pdf"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_mime_type_for_text_files() {
        let path = Path::new("test.txt");
        let content = b"Hello, world!";
        assert_eq!(detect_mime_type(path, content), "text/plain");
    }

    #[test]
    fn detect_mime_type_for_ipynb() {
        let path = Path::new("notebook.ipynb");
        let content = b"{\"cells\": []}";
        assert_eq!(detect_mime_type(path, content), "application/json");
    }

    #[test]
    fn detect_mime_type_for_png() {
        let path = Path::new("image.png");
        let content = b"\x89PNG\r\n\x1a\n";
        assert_eq!(detect_mime_type(path, content), "image/png");
    }

    #[test]
    fn detect_mime_type_for_pdf_with_magic() {
        let path = Path::new("document.pdf");
        let content = b"%PDF-1.4";
        assert_eq!(detect_mime_type(path, content), "application/pdf");
    }

    #[test]
    fn detect_mime_type_for_jpeg() {
        let path = Path::new("photo.jpg");
        let content = b"\xFF\xD8\xFF";
        assert_eq!(detect_mime_type(path, content), "image/jpeg");
    }

    #[test]
    fn is_visual_content_for_images() {
        assert!(is_visual_content("image/png"));
        assert!(is_visual_content("image/jpeg"));
        assert!(is_visual_content("image/gif"));
        assert!(is_visual_content("image/webp"));
    }

    #[test]
    fn is_visual_content_for_pdf() {
        assert!(is_visual_content("application/pdf"));
    }

    #[test]
    fn is_visual_content_for_text() {
        assert!(!is_visual_content("text/plain"));
        assert!(!is_visual_content("application/json"));
        assert!(!is_visual_content("text/html"));
    }

    #[test]
    fn truncate_line_short_line() {
        assert_eq!(truncate_line("short line", 100), "short line");
    }

    #[test]
    fn truncate_line_exact_length() {
        let line = "exactly 17 chars!";
        assert_eq!(line.len(), 17);
        assert_eq!(truncate_line(line, 17), "exactly 17 chars!");
    }

    #[test]
    fn truncate_line_long_line() {
        let line = "this is a very long line that exceeds the maximum length";
        let actual = truncate_line(line, 20);
        assert!(actual.starts_with("this is a very long"));
        assert!(actual.contains("[truncated"));
        assert!(!actual.contains("exceeds the maximum length"));
    }

    #[test]
    fn truncate_line_empty() {
        assert_eq!(truncate_line("", 100), "");
    }

    #[test]
    fn truncate_line_unicode() {
        let line = "🚀🚀🚀🚀🚀";
        let actual = truncate_line(line, 12);
        assert!(actual.contains("truncated"));
    }

    #[test]
    fn format_numbered_lines_uses_rg_style_numbers() {
        let lines = ["alpha", "beta"];
        assert_eq!(format_numbered_lines(&lines, 7, 100), "7:alpha\n8:beta");
    }

    #[tokio::test]
    async fn invoke_rejects_relative_path() {
        let tool = FileSystemRead;
        let actual = tool
            .invoke(FileSystemReadArgs {
                path: "relative/path.txt".to_string(),
                start_line: None,
                end_line: None,
            })
            .await;
        assert!(actual.is_err());
    }

    #[tokio::test]
    async fn invoke_reads_text_file() {
        let dir = std::env::temp_dir();
        let file = dir.join("scry_fs_read_test.txt");
        tokio::fs::write(&file, "line 1\nline 2\nline 3")
            .await
            .unwrap();

        let tool = FileSystemRead;
        let actual = tool
            .invoke(FileSystemReadArgs {
                path: file.to_string_lossy().into_owned(),
                start_line: None,
                end_line: None,
            })
            .await
            .unwrap();

        match actual {
            ToolResult::Text(s) => assert_eq!(s, "1:line 1\n2:line 2\n3:line 3"),
            _ => panic!("expected text result"),
        }

        let _ = tokio::fs::remove_file(file).await;
    }

    #[tokio::test]
    async fn invoke_reads_text_file_range() {
        let dir = std::env::temp_dir();
        let file = dir.join("scry_fs_read_range_test.txt");
        tokio::fs::write(&file, "a\nb\nc\nd\ne").await.unwrap();

        let tool = FileSystemRead;
        let actual = tool
            .invoke(FileSystemReadArgs {
                path: file.to_string_lossy().into_owned(),
                start_line: Some(2),
                end_line: Some(4),
            })
            .await
            .unwrap();

        match actual {
            ToolResult::Text(s) => assert_eq!(s, "2:b\n3:c\n4:d"),
            _ => panic!("expected text result"),
        }

        let _ = tokio::fs::remove_file(file).await;
    }

    #[tokio::test]
    async fn invoke_reads_pdf_as_binary() {
        let dir = std::env::temp_dir();
        let file = dir.join(format!("scry_fs_read_pdf_test_{}.pdf", std::process::id()));
        let bytes = b"%PDF-1.4\n% tiny test pdf\n";
        tokio::fs::write(&file, bytes).await.unwrap();

        let tool = FileSystemRead;
        let actual = tool
            .invoke(FileSystemReadArgs {
                path: file.to_string_lossy().into_owned(),
                start_line: None,
                end_line: None,
            })
            .await
            .unwrap();

        match actual {
            ToolResult::Binary { mime_type, data } => {
                assert_eq!(mime_type, "application/pdf");
                assert_eq!(data, bytes);
            }
            _ => panic!("expected binary result"),
        }

        let _ = tokio::fs::remove_file(file).await;
    }

    #[tokio::test]
    async fn invoke_reads_png_as_binary() {
        let dir = std::env::temp_dir();
        let file = dir.join(format!("scry_fs_read_png_test_{}.png", std::process::id()));
        let bytes = b"\x89PNG\r\n\x1a\n";
        tokio::fs::write(&file, bytes).await.unwrap();

        let tool = FileSystemRead;
        let actual = tool
            .invoke(FileSystemReadArgs {
                path: file.to_string_lossy().into_owned(),
                start_line: None,
                end_line: None,
            })
            .await
            .unwrap();

        match actual {
            ToolResult::Binary { mime_type, data } => {
                assert_eq!(mime_type, "image/png");
                assert_eq!(data, bytes);
            }
            _ => panic!("expected binary result"),
        }

        let _ = tokio::fs::remove_file(file).await;
    }

    #[tokio::test]
    async fn invoke_rejects_directories() {
        let dir =
            std::env::temp_dir().join(format!("scry_fs_read_dir_test_{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir(&dir).await.unwrap();

        let tool = FileSystemRead;
        let actual = tool
            .invoke(FileSystemReadArgs {
                path: dir.to_string_lossy().into_owned(),
                start_line: None,
                end_line: None,
            })
            .await;

        assert_eq!(
            actual.unwrap_err(),
            format!("Path is not a file: {}", dir.display())
        );

        let _ = tokio::fs::remove_dir(dir).await;
    }
}
