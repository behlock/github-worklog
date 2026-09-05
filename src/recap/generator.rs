use crate::github::Activity;
use chrono::NaiveDate;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct RecapGenerator {
    date_format: String,
}

impl RecapGenerator {
    pub fn new(date_format: &str) -> Self {
        Self {
            date_format: date_format.to_string(),
        }
    }

    pub fn date_header(&self, date: NaiveDate) -> String {
        format!("**{}**", date.format(&self.date_format))
    }

    /// Plain markdown built directly from commit subjects, grouped by repository.
    pub fn generate_markdown(&self, date: NaiveDate, activities: &[Activity]) -> String {
        let mut output = self.date_header(date);
        output.push('\n');

        if activities.is_empty() {
            output.push_str("\nNo commits found for this day.\n");
            return output;
        }

        for (repo_name, repo_activities) in group_by_repo(activities) {
            output.push_str(&format!("\n#### `{repo_name}`\n"));
            for activity in repo_activities {
                output.push_str(&format!("- {}\n", format_activity(activity)));
            }
        }

        output
    }

    /// Markdown with an LLM-produced body, normalised to the same layout as
    /// [`generate_markdown`](Self::generate_markdown). Returns `None` when
    /// nothing usable survives normalisation, so the caller can fall back to
    /// the plain recap instead of writing a header-only entry.
    pub fn generate_markdown_with_summary(&self, date: NaiveDate, summary: &str) -> Option<String> {
        let body = normalize_summary(summary);
        if body.trim().is_empty() {
            return None;
        }
        Some(format!("{}\n\n{body}\n", self.date_header(date)))
    }
}

/// Coerce LLM output into the canonical shape: strip code fences, turn any
/// heading level or bold repo line into `#### \`owner/repo\``, drop stray
/// blank lines and put exactly one blank line before each heading. Bullet
/// indentation is preserved so nested lists survive.
pub fn normalize_summary(summary: &str) -> String {
    let mut lines: Vec<&str> = summary.trim().lines().map(str::trim_end).collect();

    if lines
        .first()
        .is_some_and(|l| l.trim_start().starts_with("```"))
    {
        lines.remove(0);
    }
    if lines.last().is_some_and(|l| l.trim() == "```") {
        lines.pop();
    }

    // Group bullets under their heading, merging a heading that appears
    // more than once (models sometimes split one repo into two sections).
    // A heading that names no repository ("## Daily Summary", "### Notes")
    // is dropped, and the bullets under it are kept in a headerless section
    // rather than being attributed to the previous repository.
    let mut sections: Vec<(Option<String>, Vec<String>)> = Vec::new();
    let mut current: Option<usize> = None;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match classify(trimmed) {
            Line::RepoHeading(repo) => {
                let existing = sections
                    .iter()
                    .position(|(h, _)| h.as_deref() == Some(repo.as_str()));
                current = Some(existing.unwrap_or_else(|| {
                    sections.push((Some(repo), Vec::new()));
                    sections.len() - 1
                }));
            }
            Line::OtherHeading => {
                current = None;
            }
            Line::Text => {
                let idx = match current {
                    Some(i) => i,
                    None => {
                        sections.push((None, Vec::new()));
                        sections.len() - 1
                    }
                };
                current = Some(idx);
                sections[idx].1.push(line.to_string());
            }
        }
    }

    let mut out: Vec<String> = Vec::new();
    for (heading, bullets) in sections {
        if let Some(repo) = heading {
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push(format!("#### `{repo}`"));
        }
        out.extend(bullets);
    }
    out.join("\n")
}

enum Line {
    RepoHeading(String),
    OtherHeading,
    Text,
}

fn classify(line: &str) -> Line {
    if line.starts_with('#') {
        match heading_repo(line.trim_start_matches('#')) {
            Some(repo) => Line::RepoHeading(repo),
            None => Line::OtherHeading,
        }
    } else if let Some(inner) = line.strip_prefix("**").and_then(|l| l.strip_suffix("**")) {
        // A bold-only line is a heading only if it is exactly a repo name.
        match heading_repo(inner) {
            Some(repo) if inner.trim().trim_matches('`') == repo => Line::RepoHeading(repo),
            _ => Line::Text,
        }
    } else {
        Line::Text
    }
}

/// The `owner/repo` named by a heading's text, tolerating decoration around
/// it: backticks, bold markers, a trailing annotation such as
/// `(2 commits)` or `- frontend work`. `None` if no token looks like a repo.
fn heading_repo(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|tok| tok.trim_matches(|c: char| "`*()[]:,;".contains(c)))
        .find(|tok| looks_like_repo(tok))
        .map(str::to_string)
}

fn looks_like_repo(token: &str) -> bool {
    match token.split_once('/') {
        Some((owner, name)) => {
            !owner.is_empty()
                && !name.is_empty()
                && !name.contains('/')
                && token
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
        }
        None => false,
    }
}

fn format_activity(activity: &Activity) -> String {
    let subject = activity.commit.subject();
    match &activity.associated_pr {
        Some(pr) => format!("{subject} (`PR #{}`: {})", pr.number, pr.title),
        None => subject.to_string(),
    }
}

fn group_by_repo(activities: &[Activity]) -> BTreeMap<String, Vec<&Activity>> {
    let mut grouped: BTreeMap<String, Vec<&Activity>> = BTreeMap::new();
    for activity in activities {
        grouped
            .entry(activity.commit.repository.full_name())
            .or_default()
            .push(activity);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{CommitInfo, PullRequestInfo, RepoInfo};
    use chrono::{TimeZone, Utc};

    fn activity(owner: &str, repo: &str, message: &str, pr: Option<u64>) -> Activity {
        Activity {
            commit: CommitInfo {
                sha: "abc123".to_string(),
                message: message.to_string(),
                author: "user".to_string(),
                date: Utc.with_ymd_and_hms(2026, 1, 8, 10, 0, 0).unwrap(),
                repository: RepoInfo {
                    owner: owner.to_string(),
                    name: repo.to_string(),
                },
                html_url: format!("https://github.com/{owner}/{repo}/commit/abc123"),
            },
            associated_pr: pr.map(|n| PullRequestInfo {
                number: n,
                title: "Auth improvements".to_string(),
                state: "closed".to_string(),
                html_url: format!("https://github.com/{owner}/{repo}/pull/{n}"),
            }),
        }
    }

    #[test]
    fn generate_markdown_empty() {
        let generator = RecapGenerator::new("%d/%m/%y");
        let date = NaiveDate::from_ymd_opt(2026, 1, 8).unwrap();
        let result = generator.generate_markdown(date, &[]);
        assert_eq!(result, "**08/01/26**\n\nNo commits found for this day.\n");
    }

    #[test]
    fn generate_markdown_groups_by_repo_and_uses_first_line() {
        let generator = RecapGenerator::new("%d/%m/%y");
        let date = NaiveDate::from_ymd_opt(2026, 1, 8).unwrap();
        let activities = vec![
            activity("owner", "zeta", "Fix bug in auth\n\ndetails", Some(42)),
            activity("owner", "alpha", "Add feature", None),
        ];

        let result = generator.generate_markdown(date, &activities);
        assert_eq!(
            result,
            "**08/01/26**\n\n#### `owner/alpha`\n- Add feature\n\n#### `owner/zeta`\n- Fix bug in auth (`PR #42`: Auth improvements)\n"
        );
    }

    #[test]
    fn summary_output_matches_plain_layout() {
        let generator = RecapGenerator::new("%d/%m/%y");
        let date = NaiveDate::from_ymd_opt(2026, 1, 8).unwrap();
        let result = generator
            .generate_markdown_with_summary(date, "#### `a/b`\n- Did x")
            .unwrap();
        assert_eq!(result, "**08/01/26**\n\n#### `a/b`\n- Did x\n");
    }

    #[test]
    fn summary_that_normalises_to_nothing_is_rejected() {
        let generator = RecapGenerator::new("%d/%m/%y");
        let date = NaiveDate::from_ymd_opt(2026, 1, 8).unwrap();
        assert!(
            generator
                .generate_markdown_with_summary(date, "## Summary\n")
                .is_none()
        );
        assert!(
            generator
                .generate_markdown_with_summary(date, "```\n## Daily Summary\n```")
                .is_none()
        );
        assert!(
            generator
                .generate_markdown_with_summary(date, "   \n")
                .is_none()
        );
    }

    #[test]
    fn normalize_adds_backticks_and_blank_lines() {
        let input = "#### owner/repo\n- Did something\n#### org/lib\n- Fixed bug";
        assert_eq!(
            normalize_summary(input),
            "#### `owner/repo`\n- Did something\n\n#### `org/lib`\n- Fixed bug"
        );
    }

    #[test]
    fn normalize_preserves_existing_backticks() {
        assert_eq!(
            normalize_summary("#### `owner/repo`\n- Did something"),
            "#### `owner/repo`\n- Did something"
        );
    }

    #[test]
    fn normalize_strips_code_fences_and_other_heading_styles() {
        let input = "```markdown\n### **owner/repo**\n- One\n\n\n**org/lib**\n- Two\n```";
        assert_eq!(
            normalize_summary(input),
            "#### `owner/repo`\n- One\n\n#### `org/lib`\n- Two"
        );
    }

    #[test]
    fn normalize_keeps_nested_bullet_indentation() {
        let input = "#### `a/b`\n- Refactored auth\n  - moved token parsing\n  - added tests";
        assert_eq!(
            normalize_summary(input),
            "#### `a/b`\n- Refactored auth\n  - moved token parsing\n  - added tests"
        );
    }

    #[test]
    fn normalize_merges_repeated_headings() {
        let input = "#### `a/b`\n- One\n\n#### `c/d`\n- Two\n\n#### `a/b`\n- Three";
        assert_eq!(
            normalize_summary(input),
            "#### `a/b`\n- One\n- Three\n\n#### `c/d`\n- Two"
        );
    }

    #[test]
    fn normalize_reads_repo_out_of_annotated_headings() {
        let input = "#### `acme/api` (2 commits)\n- Fixed auth\n#### acme/web - frontend work\n- Bumped deps";
        assert_eq!(
            normalize_summary(input),
            "#### `acme/api`\n- Fixed auth\n\n#### `acme/web`\n- Bumped deps"
        );
    }

    #[test]
    fn normalize_drops_non_repo_headings_without_misattributing_bullets() {
        let input = "## Daily Summary\n#### `acme/api`\n- Fixed auth\n### Notes\n- Nothing else";
        assert_eq!(
            normalize_summary(input),
            "#### `acme/api`\n- Fixed auth\n- Nothing else"
        );
        // Bullets after a dropped heading do not inherit an unrelated repo.
        let input = "#### `acme/api`\n- Fixed auth\n#### Other work\n- Wrote docs";
        let out = normalize_summary(input);
        assert_eq!(out, "#### `acme/api`\n- Fixed auth\n- Wrote docs");
    }

    #[test]
    fn normalize_leaves_bold_prose_alone() {
        assert_eq!(
            normalize_summary("- **Important:** shipped a/b"),
            "- **Important:** shipped a/b"
        );
        assert_eq!(normalize_summary("**not a repo**"), "**not a repo**");
        assert_eq!(
            normalize_summary("**see acme/api later**"),
            "**see acme/api later**"
        );
    }

    #[test]
    fn repo_token_detection() {
        assert!(looks_like_repo("acme/api"));
        assert!(looks_like_repo("Nothing-Technology/design-machines.hack"));
        assert!(!looks_like_repo("acme/"));
        assert!(!looks_like_repo("/api"));
        assert!(!looks_like_repo("a/b/c"));
        assert!(!looks_like_repo("Daily"));
        assert_eq!(
            heading_repo(" `acme/api` (2 commits)").as_deref(),
            Some("acme/api")
        );
        assert_eq!(heading_repo("Daily Summary"), None);
    }
}
