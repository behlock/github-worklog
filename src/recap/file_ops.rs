use crate::error::{RecapError, Result};
use std::fs;
use std::path::Path;
use tracing::{debug, info, warn};

const FILE_WRITE_MAX_RETRIES: u32 = 5;

fn write_with_retry(path: &Path, content: &str) -> std::io::Result<()> {
    let mut last_err = None;
    for attempt in 0..=FILE_WRITE_MAX_RETRIES {
        match fs::write(path, content) {
            Ok(()) => return Ok(()),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    && attempt < FILE_WRITE_MAX_RETRIES =>
            {
                warn!(
                    "File write failed with EAGAIN (attempt {}/{}), retrying: {}",
                    attempt + 1,
                    FILE_WRITE_MAX_RETRIES,
                    path.display()
                );
                std::thread::sleep(std::time::Duration::from_millis(500 * (attempt as u64 + 1)));
                last_err = Some(e);
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("max retries exhausted")))
}

/// Result of attempting to write a recap
#[derive(Debug)]
pub enum WriteResult {
    /// Successfully written
    Written,
    /// Skipped because entry already exists
    Skipped,
}

/// Check if a date entry already exists in the file
/// Looks for the date header pattern: **{formatted_date}**
pub fn date_exists_in_file(path: &Path, formatted_date: &str) -> Result<bool> {
    let expanded_path = expand_tilde(path);

    let existing = match fs::read_to_string(&expanded_path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!("File does not exist yet: {}", expanded_path.display());
            return Ok(false);
        }
        Err(source) => {
            return Err(RecapError::FileOp {
                path: expanded_path.display().to_string(),
                source,
            });
        }
    };

    // Look for the date header pattern: **{formatted_date}**
    let date_header = format!("**{}**", formatted_date);
    let exists = existing.contains(&date_header);

    if exists {
        debug!("Found existing entry for date '{}' in file", formatted_date);
    }

    Ok(exists)
}

/// Prepend content to file with duplicate detection
///
/// # Arguments
/// * `path` - Output file path
/// * `content` - Markdown content to write
/// * `formatted_date` - The formatted date string to check for duplicates
/// * `force` - If true, overwrite existing entry for this date
///
/// # Returns
/// * `Ok(WriteResult::Written)` - Content was written
/// * `Ok(WriteResult::Skipped)` - Entry already exists (when force=false)
/// * `Err(RecapError::DuplicateDate)` - Entry exists and force=false (alternative error mode)
pub fn prepend_to_file(
    path: &Path,
    content: &str,
    formatted_date: &str,
    force: bool,
) -> Result<WriteResult> {
    let expanded_path = expand_tilde(path);

    // Ensure parent directory exists
    if let Some(parent) = expanded_path.parent() {
        fs::create_dir_all(parent).map_err(|source| RecapError::FileOp {
            path: parent.display().to_string(),
            source,
        })?;
    }

    // Read existing content (or empty string if file doesn't exist)
    let existing = fs::read_to_string(&expanded_path).unwrap_or_default();

    // Check for duplicate date entry
    let date_header = format!("**{}**", formatted_date);
    if existing.contains(&date_header) {
        if force {
            warn!(
                "Overwriting existing entry for date '{}' (--force specified)",
                formatted_date
            );
            // Remove the existing entry for this date before prepending
            let cleaned = remove_date_entry(&existing, &date_header);
            let separator = if cleaned.is_empty() { "" } else { "\n---\n\n" };
            let new_content = format!("{}{}{}", content, separator, cleaned);

            write_with_retry(&expanded_path, &new_content).map_err(|source| {
                RecapError::FileOp {
                    path: expanded_path.display().to_string(),
                    source,
                }
            })?;

            info!(
                "Replaced entry for date '{}' in {}",
                formatted_date,
                expanded_path.display()
            );
            return Ok(WriteResult::Written);
        } else {
            info!(
                "Skipping: entry for date '{}' already exists in {}",
                formatted_date,
                expanded_path.display()
            );
            return Err(RecapError::DuplicateDate {
                date: formatted_date.to_string(),
                path: expanded_path.display().to_string(),
            });
        }
    }

    // Create new content with prepended recap (newest first)
    let separator = if existing.is_empty() { "" } else { "\n---\n\n" };
    let new_content = format!("{}{}{}", content, separator, existing);

    // Write back
    write_with_retry(&expanded_path, &new_content).map_err(|source| RecapError::FileOp {
        path: expanded_path.display().to_string(),
        source,
    })?;

    info!(
        "Written recap for date '{}' to {}",
        formatted_date,
        expanded_path.display()
    );
    Ok(WriteResult::Written)
}

/// Remove an existing date entry from the file content
/// Removes from the date header until the next separator (---) or end of file
fn remove_date_entry(content: &str, date_header: &str) -> String {
    let Some(start) = content.find(date_header) else {
        return content.to_string();
    };

    // Find the end of this entry (next separator or end of file)
    let after_header = &content[start..];
    let end = if let Some(sep_pos) = after_header.find("\n---\n") {
        // Include the separator in what we remove, but keep content after it
        start + sep_pos + 5 // "\n---\n" is 5 chars
    } else {
        content.len()
    };

    // Also handle leading separator if this wasn't the first entry
    let actual_start = if start > 0 {
        // Check if there's a separator before this entry
        let before = &content[..start];
        if before.ends_with("\n---\n\n") {
            start - 6 // Remove the leading "\n---\n\n"
        } else {
            start
        }
    } else {
        start
    };

    let mut result = String::new();
    result.push_str(&content[..actual_start]);
    if end < content.len() {
        // Skip any leading newlines after the separator
        let remaining = content[end..].trim_start_matches('\n');
        if !remaining.is_empty() && !result.is_empty() {
            result.push_str("\n---\n\n");
        }
        result.push_str(remaining);
    }

    result
}

/// Legacy function for backwards compatibility - use prepend_to_file instead
#[deprecated(since = "0.2.0", note = "Use prepend_to_file with duplicate detection")]
pub fn append_to_file(path: &Path, content: &str) -> Result<()> {
    // Expand ~ to home directory
    let expanded_path = expand_tilde(path);

    // Ensure parent directory exists
    if let Some(parent) = expanded_path.parent() {
        fs::create_dir_all(parent).map_err(|source| RecapError::FileOp {
            path: parent.display().to_string(),
            source,
        })?;
    }

    // Read existing content (or empty string if file doesn't exist)
    let existing = fs::read_to_string(&expanded_path).unwrap_or_default();

    // Create new content with prepended recap (newest first)
    let separator = if existing.is_empty() { "" } else { "\n---\n\n" };
    let new_content = format!("{}{}{}", content, separator, existing);

    // Write back
    write_with_retry(&expanded_path, &new_content).map_err(|source| RecapError::FileOp {
        path: expanded_path.display().to_string(),
        source,
    })?;

    Ok(())
}

pub fn read_file(path: &Path) -> Result<String> {
    let expanded_path = expand_tilde(path);
    fs::read_to_string(&expanded_path).map_err(|source| RecapError::FileOp {
        path: expanded_path.display().to_string(),
        source,
    })
}

fn expand_tilde(path: &Path) -> std::path::PathBuf {
    let path_str = path.to_string_lossy();
    if path_str.starts_with("~") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path_str[2..]);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    #[allow(deprecated)]
    fn test_append_to_new_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.md");

        append_to_file(&file_path, "New content").unwrap();

        let contents = fs::read_to_string(&file_path).unwrap();
        assert_eq!(contents, "New content");
    }

    #[test]
    #[allow(deprecated)]
    fn test_prepend_to_existing_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.md");

        fs::write(&file_path, "Old content").unwrap();
        append_to_file(&file_path, "New content").unwrap();

        let contents = fs::read_to_string(&file_path).unwrap();
        assert!(contents.starts_with("New content"));
        assert!(contents.contains("---"));
        assert!(contents.ends_with("Old content"));
    }

    #[test]
    fn test_prepend_to_file_new_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.md");

        let result = prepend_to_file(&file_path, "**16/01/26**\n\nNew content", "16/01/26", false);
        assert!(matches!(result, Ok(WriteResult::Written)));

        let contents = fs::read_to_string(&file_path).unwrap();
        assert!(contents.contains("**16/01/26**"));
        assert!(contents.contains("New content"));
    }

    #[test]
    fn test_prepend_to_file_duplicate_detection() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.md");

        // Write initial entry
        fs::write(&file_path, "**16/01/26**\n\nExisting content").unwrap();

        // Try to add same date without force - should fail
        let result = prepend_to_file(&file_path, "**16/01/26**\n\nNew content", "16/01/26", false);
        assert!(matches!(result, Err(RecapError::DuplicateDate { .. })));

        // Original content should be unchanged
        let contents = fs::read_to_string(&file_path).unwrap();
        assert!(contents.contains("Existing content"));
        assert!(!contents.contains("New content"));
    }

    #[test]
    fn test_prepend_to_file_force_overwrite() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.md");

        // Write initial entry
        fs::write(&file_path, "**16/01/26**\n\nExisting content").unwrap();

        // Force overwrite with same date
        let result = prepend_to_file(&file_path, "**16/01/26**\n\nNew content", "16/01/26", true);
        assert!(matches!(result, Ok(WriteResult::Written)));

        // New content should replace old
        let contents = fs::read_to_string(&file_path).unwrap();
        assert!(contents.contains("New content"));
        assert!(!contents.contains("Existing content"));
    }

    #[test]
    fn test_prepend_to_file_different_date() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.md");

        // Write initial entry
        fs::write(&file_path, "**15/01/26**\n\nOld content").unwrap();

        // Add different date - should succeed
        let result = prepend_to_file(&file_path, "**16/01/26**\n\nNew content", "16/01/26", false);
        assert!(matches!(result, Ok(WriteResult::Written)));

        let contents = fs::read_to_string(&file_path).unwrap();
        assert!(contents.contains("**16/01/26**"));
        assert!(contents.contains("**15/01/26**"));
        assert!(contents.contains("New content"));
        assert!(contents.contains("Old content"));
        // New content should be first (prepended)
        assert!(contents.find("**16/01/26**") < contents.find("**15/01/26**"));
    }

    #[test]
    fn test_date_exists_in_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.md");

        // File doesn't exist yet
        assert!(!date_exists_in_file(&file_path, "16/01/26").unwrap());

        // Create file with date
        fs::write(&file_path, "**16/01/26**\n\nContent").unwrap();
        assert!(date_exists_in_file(&file_path, "16/01/26").unwrap());
        assert!(!date_exists_in_file(&file_path, "15/01/26").unwrap());
    }

    #[test]
    fn test_remove_date_entry_first() {
        let content = "**16/01/26**\n\nFirst entry\n---\n\n**15/01/26**\n\nSecond entry";
        let result = remove_date_entry(content, "**16/01/26**");
        assert!(!result.contains("First entry"));
        assert!(result.contains("**15/01/26**"));
        assert!(result.contains("Second entry"));
    }

    #[test]
    fn test_remove_date_entry_middle() {
        let content =
            "**17/01/26**\n\nFirst\n---\n\n**16/01/26**\n\nMiddle\n---\n\n**15/01/26**\n\nLast";
        let result = remove_date_entry(content, "**16/01/26**");
        assert!(result.contains("**17/01/26**"));
        assert!(result.contains("First"));
        assert!(!result.contains("Middle"));
        assert!(result.contains("**15/01/26**"));
        assert!(result.contains("Last"));
    }

    #[test]
    fn test_remove_date_entry_only() {
        let content = "**16/01/26**\n\nOnly entry";
        let result = remove_date_entry(content, "**16/01/26**");
        assert!(result.is_empty() || result.trim().is_empty());
    }
}
