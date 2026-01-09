use crate::error::{RecapError, Result};
use std::fs;
use std::path::Path;

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

    // Create new content with appended recap (chronological order)
    let separator = if existing.is_empty() { "" } else { "\n---\n\n" };
    let new_content = format!("{}{}{}", existing, separator, content);

    // Write back
    fs::write(&expanded_path, new_content).map_err(|source| RecapError::FileOp {
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

pub fn copy_file(source: &Path, dest: &Path) -> Result<()> {
    let source_expanded = expand_tilde(source);
    let dest_expanded = expand_tilde(dest);

    // Ensure destination parent directory exists
    if let Some(parent) = dest_expanded.parent() {
        fs::create_dir_all(parent).map_err(|source| RecapError::FileOp {
            path: parent.display().to_string(),
            source,
        })?;
    }

    fs::copy(&source_expanded, &dest_expanded).map_err(|source| RecapError::FileOp {
        path: dest_expanded.display().to_string(),
        source,
    })?;

    Ok(())
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
    fn test_append_to_new_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.md");

        append_to_file(&file_path, "New content").unwrap();

        let contents = fs::read_to_string(&file_path).unwrap();
        assert_eq!(contents, "New content");
    }

    #[test]
    fn test_append_to_existing_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.md");

        fs::write(&file_path, "Old content").unwrap();
        append_to_file(&file_path, "New content").unwrap();

        let contents = fs::read_to_string(&file_path).unwrap();
        assert!(contents.starts_with("Old content"));
        assert!(contents.contains("---"));
        assert!(contents.ends_with("New content"));
    }
}
