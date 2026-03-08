use crate::github::Activity;
use chrono::NaiveDate;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct RecapGenerator {
    date_format: String,
}

impl RecapGenerator {
    pub fn new(date_format: &str) -> Self {
        Self {
            date_format: date_format.to_string(),
        }
    }

    pub fn generate_markdown(&self, date: NaiveDate, activities: Vec<Activity>) -> String {
        let mut output = String::new();

        // Bold date header in configured format (default: DD/MM/YY)
        let formatted_date = date.format(&self.date_format).to_string();
        output.push_str(&format!("**{}**\n", formatted_date));

        if activities.is_empty() {
            output.push_str("\nNo commits found for this day.\n");
            return output;
        }

        // Group activities by repository
        let grouped = self.group_by_repo(&activities);

        for (repo_name, repo_activities) in grouped {
            output.push_str(&format!("\n#### `{}`\n", repo_name));

            for activity in repo_activities {
                let bullet = self.format_activity(activity);
                output.push_str(&format!("- {}\n", bullet));
            }
        }

        output
    }

    /// Generate markdown with a pre-summarized content (from Claude)
    pub fn generate_markdown_with_summary(&self, date: NaiveDate, summary: &str) -> String {
        let mut output = String::new();

        // Bold date header in configured format (default: DD/MM/YY)
        let formatted_date = date.format(&self.date_format).to_string();
        output.push_str(&format!("**{}**\n\n", formatted_date));

        // Add the summarized bullet points
        output.push_str(summary);
        output.push('\n');

        output
    }

    fn format_activity(&self, activity: &Activity) -> String {
        // Get first line of commit message
        let commit_msg = activity.commit.message.lines().next().unwrap_or("").trim();

        match &activity.associated_pr {
            Some(pr) => {
                format!("{} (`PR #{}`: {})", commit_msg, pr.number, pr.title)
            }
            None => commit_msg.to_string(),
        }
    }

    fn group_by_repo<'a>(&self, activities: &'a [Activity]) -> BTreeMap<String, Vec<&'a Activity>> {
        let mut grouped: BTreeMap<String, Vec<&'a Activity>> = BTreeMap::new();

        for activity in activities {
            let repo_name = format!(
                "{}/{}",
                activity.commit.repository.owner, activity.commit.repository.name
            );
            grouped.entry(repo_name).or_default().push(activity);
        }

        grouped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{CommitInfo, PullRequestInfo, RepoInfo};
    use chrono::{TimeZone, Utc};

    #[test]
    fn test_generate_markdown_empty() {
        let generator = RecapGenerator::new("%d/%m/%y");
        let date = NaiveDate::from_ymd_opt(2026, 1, 8).unwrap();
        let result = generator.generate_markdown(date, vec![]);

        assert!(result.contains("**08/01/26**"));
        assert!(result.contains("No commits found"));
    }

    #[test]
    fn test_generate_markdown_with_activities() {
        let generator = RecapGenerator::new("%d/%m/%y");
        let date = NaiveDate::from_ymd_opt(2026, 1, 8).unwrap();

        let activities = vec![Activity {
            commit: CommitInfo {
                sha: "abc123".to_string(),
                message: "Fix bug in auth".to_string(),
                author: "user".to_string(),
                date: Utc.with_ymd_and_hms(2026, 1, 8, 10, 0, 0).unwrap(),
                repository: RepoInfo {
                    owner: "owner".to_string(),
                    name: "repo".to_string(),
                },
                html_url: "https://github.com/owner/repo/commit/abc123".to_string(),
            },
            associated_pr: Some(PullRequestInfo {
                number: 42,
                title: "Auth improvements".to_string(),
                state: "merged".to_string(),
                html_url: "https://github.com/owner/repo/pull/42".to_string(),
            }),
        }];

        let result = generator.generate_markdown(date, activities);

        assert!(result.contains("**08/01/26**"));
        assert!(result.contains("#### `owner/repo`"));
        assert!(result.contains("Fix bug in auth (`PR #42`: Auth improvements)"));
    }
}
