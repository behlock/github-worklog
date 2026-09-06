//! Persistence for the worklog markdown file.
//!
//! The file is a newest-first sequence of entries. Every entry starts with a
//! bold date header line (`**16/01/26**`) and entries are separated by a
//! horizontal rule:
//!
//! ```text
//! **16/01/26**
//!
//! #### `owner/repo`
//! - Did something
//!
//! ---
//!
//! **15/01/26**
//! ...
//! ```
//!
//! Entries are located by their header *line*, and entries are compared by
//! the *date* the header denotes, so a `**bold**` remark, a date mentioned in
//! a bullet, a `---` inside a summary, or a change of `DATE_FORMAT` cannot
//! confuse the parser.

use crate::error::{RecapError, Result};
use chrono::NaiveDate;
use std::fs;
use std::path::Path;
use tracing::{debug, info, warn};

const ENTRY_SEPARATOR: &str = "\n\n---\n\n";
/// Transient EAGAIN failures have been observed on cloud-synced folders
/// (iCloud Drive); retry a handful of times with a growing pause.
const WRITE_MAX_RETRIES: u32 = 5;

/// Formats tried, after the configured one, when reading headers written by
/// an earlier configuration.
const COMMON_DATE_FORMATS: &[&str] = &[
    "%d/%m/%y",
    "%Y-%m-%d",
    "%d/%m/%Y",
    "%m/%d/%y",
    "%m/%d/%Y",
    "%d-%m-%Y",
    "%d.%m.%Y",
    "%d.%m.%y",
    "%d %b %Y",
    "%d %B %Y",
    "%b %d, %Y",
    "%B %d, %Y",
    "%A %d %B %Y",
    "%A, %d %B %Y",
    "%a %d %b %Y",
    "%Y/%m/%d",
    "%Y%m%d",
];

/// Outcome of attempting to write a recap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteResult {
    /// The entry was written (or replaced, when forced).
    Written,
    /// An entry for this date already exists and `force` was not set.
    Skipped,
}

/// Recognises entry header lines (`**<date>**`) and extracts their date.
#[derive(Debug, Clone, Copy)]
pub struct HeaderMatcher<'a> {
    date_format: &'a str,
}

impl<'a> HeaderMatcher<'a> {
    pub fn new(date_format: &'a str) -> Self {
        Self { date_format }
    }

    /// The text between the bold markers, if the line is bold-only.
    fn bold_inner(line: &str) -> Option<&str> {
        let line = line.trim_end();
        line.strip_prefix("**")?
            .strip_suffix("**")
            .map(str::trim)
            .filter(|inner| !inner.is_empty() && !inner.contains("**"))
    }

    /// The date a header line denotes, trying the configured format first
    /// and then formats an earlier configuration might have used.
    fn header_date(&self, line: &str) -> Option<NaiveDate> {
        let inner = Self::bold_inner(line)?;
        std::iter::once(self.date_format)
            .chain(COMMON_DATE_FORMATS.iter().copied())
            .find_map(|fmt| NaiveDate::parse_from_str(inner, fmt).ok())
    }

    /// Whether a line starts an entry. Besides parseable dates, accept a
    /// short bold-only line that starts with a digit (a date in a format we
    /// do not know) so such entries are never swallowed into a neighbour.
    fn is_header(&self, line: &str) -> bool {
        if self.header_date(line).is_some() {
            return true;
        }
        Self::bold_inner(line).is_some_and(|inner| {
            inner.len() <= 32
                && inner.starts_with(|c: char| c.is_ascii_digit())
                && !inner.contains('`')
        })
    }

    fn entry_date(&self, entry: &str) -> Option<NaiveDate> {
        self.header_date(entry.lines().next()?)
    }

    fn has_header(&self, entry: &str) -> bool {
        entry.lines().next().is_some_and(|l| self.is_header(l))
    }
}

/// Prepend `content` (an entry whose header denotes `date`) to the worklog.
///
/// If an entry for the same date already exists, whatever format its header
/// uses, it is left untouched and `Skipped` is returned, unless `force` is
/// set, in which case the old entry is removed first.
pub fn prepend_to_file(
    path: &Path,
    content: &str,
    date: NaiveDate,
    matcher: HeaderMatcher<'_>,
    force: bool,
) -> Result<WriteResult> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|source| file_err(parent, source))?;
    }

    let existing = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => return Err(file_err(path, source)),
    };

    let mut entries = split_entries(&existing, matcher);
    let had_existing = entries.iter().any(|e| matcher.entry_date(e) == Some(date));

    if had_existing {
        if !force {
            info!(
                "Skipping: entry for {date} already exists in {}",
                path.display()
            );
            return Ok(WriteResult::Skipped);
        }
        warn!("Overwriting existing entry for {date} (--force specified)");
        entries.retain(|e| matcher.entry_date(e) != Some(date));
    }

    // A leading chunk with no header (a title, say) stays at the top.
    let insert_at = match entries.first() {
        Some(first) if !matcher.has_header(first) => 1,
        _ => 0,
    };
    entries.insert(insert_at, content);
    let new_content = join_entries(&entries);

    write_atomically(path, &new_content)?;

    info!(
        "{} entry for {date} in {}",
        if had_existing { "Replaced" } else { "Written" },
        path.display()
    );
    Ok(WriteResult::Written)
}

/// Split file content into entries. An entry begins at a header line and runs
/// until the next header line; separator lines between entries are dropped.
/// Any text before the first header is kept as its own chunk.
fn split_entries<'c>(content: &'c str, matcher: HeaderMatcher<'_>) -> Vec<&'c str> {
    let mut starts: Vec<usize> = Vec::new();
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        if matcher.is_header(line) {
            starts.push(offset);
        }
        offset += line.len();
    }

    let mut bounds = Vec::with_capacity(starts.len() + 1);
    if starts.first().is_none_or(|&s| s > 0) {
        bounds.push(0);
    }
    bounds.extend(starts);
    bounds.push(content.len());

    bounds
        .windows(2)
        .map(|w| strip_separators(&content[w[0]..w[1]]))
        .filter(|e| !e.is_empty())
        .collect()
}

/// A markdown thematic break made of dashes: `---`, `----`, with optional
/// surrounding whitespace.
fn is_rule_line(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && t.bytes().all(|b| b == b'-')
}

/// Trim surrounding blank lines and any trailing rule *lines* from an entry,
/// leaving the entry's own text (including a `---` inside a bullet) intact.
fn strip_separators(entry: &str) -> &str {
    let mut s = entry.trim_matches(['\n', '\r']);
    loop {
        let last_start = s.rfind('\n').map_or(0, |i| i + 1);
        if is_rule_line(&s[last_start..]) {
            s = s[..last_start].trim_end_matches(['\n', '\r']);
        } else {
            return s;
        }
    }
}

fn join_entries(entries: &[&str]) -> String {
    let mut out = entries
        .iter()
        .map(|e| strip_separators(e))
        .collect::<Vec<_>>()
        .join(ENTRY_SEPARATOR);
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Write via a temporary file in the same directory plus rename, so a crash
/// mid-write can never leave a truncated worklog behind.
///
/// Symlinks are followed so the real file is replaced rather than the link,
/// the existing file's permissions are kept, and if the directory does not
/// allow a rename (but the file itself is writable) we fall back to writing
/// in place.
fn write_atomically(path: &Path, content: &str) -> Result<()> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let file_name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "worklog.md".to_string());
    let tmp_path = target.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    let existing_perms = fs::metadata(&target).ok().map(|m| m.permissions());

    let result = retry_on_eagain(|| {
        fs::write(&tmp_path, content)?;
        if let Some(perms) = &existing_perms {
            fs::set_permissions(&tmp_path, perms.clone())?;
        }
        fs::rename(&tmp_path, &target)
    });

    let result = match result {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            let _ = fs::remove_file(&tmp_path);
            warn!(
                "Cannot replace {} atomically ({e}); writing in place",
                target.display()
            );
            retry_on_eagain(|| fs::write(&target, content))
        }
        other => other,
    };

    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result.map_err(|source| file_err(&target, source))
}

fn retry_on_eagain(mut op: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut attempt = 0u32;
    loop {
        match op() {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && attempt < WRITE_MAX_RETRIES => {
                attempt += 1;
                warn!(
                    "File write returned EAGAIN (attempt {attempt}/{WRITE_MAX_RETRIES}), retrying"
                );
                std::thread::sleep(std::time::Duration::from_millis(500 * u64::from(attempt)));
            }
            Err(e) => {
                debug!("File write failed: {e}");
                return Err(e);
            }
        }
    }
}

fn file_err(path: &Path, source: std::io::Error) -> RecapError {
    RecapError::FileOp {
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const ENTRY_A: &str = "**16/01/26**\n\n#### `a/b`\n- New content\n";
    const ENTRY_B: &str = "**15/01/26**\n\n#### `a/b`\n- Old content\n";
    const FMT: HeaderMatcher<'static> = HeaderMatcher {
        date_format: "%d/%m/%y",
    };

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn prepend(path: &Path, content: &str, date: NaiveDate, force: bool) -> Result<WriteResult> {
        prepend_to_file(path, content, date, FMT, force)
    }

    #[test]
    fn writes_new_file_and_creates_parent_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("test.md");

        let result = prepend(&path, ENTRY_A, d(2026, 1, 16), false).unwrap();
        assert_eq!(result, WriteResult::Written);
        assert_eq!(read(&path), ENTRY_A);
    }

    #[test]
    fn prepends_newest_first_with_separator() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        fs::write(&path, ENTRY_B).unwrap();

        prepend(&path, ENTRY_A, d(2026, 1, 16), false).unwrap();
        assert_eq!(
            read(&path),
            "**16/01/26**\n\n#### `a/b`\n- New content\n\n---\n\n**15/01/26**\n\n#### `a/b`\n- Old content\n"
        );
    }

    #[test]
    fn skips_duplicate_without_force() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        fs::write(&path, ENTRY_A).unwrap();

        let result = prepend(&path, "**16/01/26**\n\n- Other\n", d(2026, 1, 16), false).unwrap();
        assert_eq!(result, WriteResult::Skipped);
        assert_eq!(read(&path), ENTRY_A);
    }

    #[test]
    fn force_replaces_entry_in_place_of_first_position() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        let original = join_entries(&["**17/01/26**\n- Newer", ENTRY_A, ENTRY_B]);
        fs::write(&path, &original).unwrap();

        let replacement = "**16/01/26**\n\n- Replaced\n";
        let result = prepend(&path, replacement, d(2026, 1, 16), true).unwrap();
        assert_eq!(result, WriteResult::Written);

        let content = read(&path);
        assert!(content.starts_with("**16/01/26**\n\n- Replaced\n\n---\n\n**17/01/26**"));
        assert!(!content.contains("New content"));
        assert!(content.contains("Old content"));
        assert_eq!(content.matches("**16/01/26**").count(), 1);
    }

    #[test]
    fn date_inside_a_bullet_is_not_a_duplicate() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        fs::write(
            &path,
            "**15/01/26**\n\n- Backfilled the **16/01/26** report\n",
        )
        .unwrap();

        let result = prepend(&path, ENTRY_A, d(2026, 1, 16), false).unwrap();
        assert_eq!(result, WriteResult::Written);
        assert!(read(&path).contains("Backfilled"));
    }

    #[test]
    fn bold_prose_inside_an_entry_is_not_a_header() {
        let content = "**16/01/26**\n\n#### `a/b`\n- One\n\n**Highlights**\n- Two\n\n**owner/repo**\n- Three\n";
        let entries = split_entries(content, FMT);
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(join_entries(&entries), content);

        // --force removes the whole entry, leaving no orphaned tail.
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        fs::write(&path, content).unwrap();
        prepend(&path, "**16/01/26**\n\n- Replaced\n", d(2026, 1, 16), true).unwrap();
        assert_eq!(read(&path), "**16/01/26**\n\n- Replaced\n");
    }

    #[test]
    fn headers_in_old_formats_are_still_recognised_and_deduplicated() {
        // File written with the default format; user has since switched to ISO.
        let iso = HeaderMatcher::new("%Y-%m-%d");
        let content = "**16/01/26**\n\n- old style\n\n---\n\n**15/01/26**\n\n- older\n";
        let entries = split_entries(content, iso);
        assert_eq!(entries.len(), 2, "{entries:?}");
        assert_eq!(iso.entry_date(entries[0]), Some(d(2026, 1, 16)));

        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        fs::write(&path, content).unwrap();

        // New entry goes on top, not under a "preamble".
        prepend_to_file(
            &path,
            "**2026-01-17**\n\n- new style\n",
            d(2026, 1, 17),
            iso,
            false,
        )
        .unwrap();
        assert!(read(&path).starts_with("**2026-01-17**\n\n- new style\n\n---\n\n**16/01/26**"));

        // Same day in the old format is detected as a duplicate...
        let r = prepend_to_file(
            &path,
            "**2026-01-16**\n\n- dup\n",
            d(2026, 1, 16),
            iso,
            false,
        )
        .unwrap();
        assert_eq!(r, WriteResult::Skipped);
        // ...and --force replaces it rather than adding a second one.
        prepend_to_file(
            &path,
            "**2026-01-16**\n\n- replaced\n",
            d(2026, 1, 16),
            iso,
            true,
        )
        .unwrap();
        let content = read(&path);
        assert!(!content.contains("old style"));
        assert!(content.contains("- replaced"));
        assert_eq!(
            content.matches("01/26**").count() + content.matches("2026-01-16").count(),
            2
        );
    }

    #[test]
    fn unknown_digit_led_headers_are_kept_separate() {
        let content = "**16.Jan.26**\n\n- a\n\n---\n\n**15.Jan.26**\n\n- b\n";
        let entries = split_entries(content, FMT);
        assert_eq!(entries.len(), 2, "{entries:?}");
        assert_eq!(join_entries(&entries), content);
    }

    #[test]
    fn header_must_look_like_a_date() {
        assert!(FMT.is_header("**16/01/26**"));
        assert!(FMT.is_header("**16/01/26**  "));
        assert!(FMT.is_header("**2026-01-16**"));
        assert!(FMT.is_header("**Friday 16 January 2026**"));
        assert!(!FMT.is_header("**Highlights**"));
        assert!(!FMT.is_header("**acme/api2**"));
        assert!(!FMT.is_header("**16/01/26** trailing"));
        assert_eq!(FMT.header_date("**16/01/26**"), Some(d(2026, 1, 16)));
        // Configured format wins over the ambiguous US reading.
        assert_eq!(FMT.header_date("**03/04/26**"), Some(d(2026, 4, 3)));
        assert_eq!(
            HeaderMatcher::new("%m/%d/%y").header_date("**03/04/26**"),
            Some(d(2026, 3, 4))
        );
    }

    #[test]
    fn rule_inside_an_entry_does_not_split_it() {
        let content = "**16/01/26**\n\n- a\n\n---\n\n- b\n\n---\n\n**15/01/26**\n\n- c\n";
        let entries = split_entries(content, FMT);
        assert_eq!(
            entries,
            vec!["**16/01/26**\n\n- a\n\n---\n\n- b", "**15/01/26**\n\n- c"]
        );
    }

    #[test]
    fn separator_stripping_only_removes_whole_rule_lines() {
        assert_eq!(
            strip_separators("- Removed the legacy ---\n"),
            "- Removed the legacy ---"
        );
        assert_eq!(strip_separators("- a\n\n----\n"), "- a");
        assert_eq!(strip_separators("- a\n\n---  \n\n"), "- a");
        assert_eq!(strip_separators("- a\r\n\r\n---\r\n"), "- a");
        assert_eq!(strip_separators("- a\n---\n---\n"), "- a");
        assert_eq!(strip_separators("---\n"), "");
        // Idempotent: reassembling an existing file changes nothing.
        let content = "**16/01/26**\n\n- ends with ---\n\n---\n\n**15/01/26**\n\n- b\n";
        assert_eq!(join_entries(&split_entries(content, FMT)), content);
    }

    #[test]
    fn split_and_join_round_trip() {
        let original = join_entries(&[ENTRY_A, ENTRY_B]);
        assert_eq!(join_entries(&split_entries(&original, FMT)), original);
        assert!(split_entries("", FMT).is_empty());
        assert_eq!(join_entries(&[]), "");
    }

    #[test]
    fn preamble_before_first_header_is_preserved() {
        let content = "# My worklog\n\n**15/01/26**\n\n- c\n";
        let entries = split_entries(content, FMT);
        assert_eq!(entries, vec!["# My worklog", "**15/01/26**\n\n- c"]);
    }

    #[test]
    fn new_entry_goes_below_a_preamble_title() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        fs::write(&path, "# My worklog\n\n---\n\n**15/01/26**\n\n- c\n").unwrap();

        prepend(&path, ENTRY_A, d(2026, 1, 16), false).unwrap();
        assert_eq!(
            read(&path),
            "# My worklog\n\n---\n\n**16/01/26**\n\n#### `a/b`\n- New content\n\n---\n\n**15/01/26**\n\n- c\n"
        );
        prepend(&path, "**17/01/26**\n- d\n", d(2026, 1, 17), false).unwrap();
        prepend(&path, "**16/01/26**\n- e\n", d(2026, 1, 16), true).unwrap();
        let content = read(&path);
        assert!(
            content.starts_with("# My worklog\n\n---\n\n**16/01/26**\n- e\n\n---\n\n**17/01/26**")
        );
        assert_eq!(content.matches("# My worklog").count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_symlinks_and_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let real = dir.path().join("real.md");
        let link = dir.path().join("link.md");
        fs::write(&real, "old").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write_atomically(&link, "new").unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
        assert_eq!(
            fs::metadata(&real).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn atomic_write_leaves_no_temp_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.md");
        write_atomically(&path, "hello").unwrap();
        assert_eq!(read(&path), "hello");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
