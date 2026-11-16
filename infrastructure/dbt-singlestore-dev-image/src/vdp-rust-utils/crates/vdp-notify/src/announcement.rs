//! The release candidate overview: a port of `ReleaseEmailBuilder` (`email_service.py`), byte for byte, so the
//! announcement people receive does not change with the backend.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use vdp_release::{Analysis, AnalysisKind, TicketEntry};

/// One ticket line of the overview.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AnnouncementRow {
    /// `INTEL-1234` or `hotfix/<slug>`.
    pub id: String,
    /// The Jira page; empty for hotfix/bugfix branches.
    pub url: String,
    /// The Jira summary, else the subject of its latest merge.
    pub summary: String,
    /// The fix versions, comma-separated.
    pub fix_version: String,
    pub status: String,
    /// Author of its latest merge.
    pub committed_by: String,
    pub reporter: String,
}

impl AnnouncementRow {
    pub fn from_ticket(ticket: &TicketEntry) -> Self {
        let latest = ticket.merges.last().or(ticket.reverted_on_compare.as_ref());
        let subject = latest.map(|c| c.subject.clone()).unwrap_or_default();
        let jira = ticket.jira.as_ref();
        Self {
            id: ticket.id.clone(),
            url: jira.map(|j| j.url.clone()).unwrap_or_default(),
            summary: jira
                .map(|j| j.summary.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or(subject),
            fix_version: jira.map(|j| j.fix_versions.join(", ")).unwrap_or_default(),
            status: jira.map(|j| j.status.clone()).unwrap_or_default(),
            committed_by: latest.map(|c| c.author.clone()).unwrap_or_default(),
            reporter: jira.and_then(|j| j.reporter.clone()).unwrap_or_default(),
        }
    }
}

/// A rendered announcement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Announcement {
    pub subject: String,
    pub html: String,
    pub text: String,
    pub included: Vec<AnnouncementRow>,
    pub excluded: Vec<AnnouncementRow>,
}

impl Announcement {
    /// The overview of an analysis with a selection: for a release the selection is what gets reverted (so
    /// excluded), for a patch what gets added (so included). Only selectable tickets are listed: a release lists
    /// the tickets merged since the merge-base, a patch the ones the release branch lacks.
    pub fn for_analysis(analysis: &Analysis, selection: &BTreeSet<String>, branch: &str) -> Self {
        let mut included = Vec::new();
        let mut excluded = Vec::new();
        for ticket in analysis.tickets.iter().filter(|t| t.selectable) {
            let selected = selection.contains(&ticket.id);
            let goes_in = match analysis.request.kind {
                AnalysisKind::Release => !selected,
                AnalysisKind::Patch => selected,
            };
            let row = AnnouncementRow::from_ticket(ticket);
            if goes_in {
                included.push(row);
            } else {
                excluded.push(row);
            }
        }
        Self::render(&analysis.request.fix_version, branch, included, excluded)
    }

    pub fn render(
        fix_version: &str,
        branch: &str,
        included: Vec<AnnouncementRow>,
        excluded: Vec<AnnouncementRow>,
    ) -> Self {
        let title = if !fix_version.is_empty() && !branch.is_empty() && fix_version != branch {
            format!("{fix_version} ({branch})")
        } else if !fix_version.is_empty() {
            fix_version.to_owned()
        } else if !branch.is_empty() {
            branch.to_owned()
        } else {
            "N/A".to_owned()
        };
        Self {
            subject: format!("🚀 Release Candidate Overview: {title}"),
            html: html(fix_version, branch, &included, &excluded),
            text: text(fix_version, branch, &included, &excluded),
            included,
            excluded,
        }
    }
}

/// Python's `html.escape(s, quote=True)`.
pub(crate) fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

fn or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() { fallback } else { value }
}

fn row_html(row: &AnnouncementRow) -> String {
    let id = escape(or(&row.id, "-"));
    let url = escape(&row.url);
    let message = escape(&row.summary);
    let fix_version = escape(or(&row.fix_version, "None"));
    let status = escape(or(&row.status, "Unknown"));
    let author = escape(or(&row.committed_by, "Unknown"));
    let reporter = escape(or(&row.reporter, "Unknown"));
    format!(
        r#"
            <tr>
              <td style="padding: 10px 12px; border-bottom: 1px solid #e2e8f0; font-weight: 600;">
                <a href="{url}" style="color: #2563eb; text-decoration: none;">{id}</a>
              </td>
              <td style="padding: 10px 12px; border-bottom: 1px solid #e2e8f0; font-size: 13px; max-width: 320px; word-break: break-word;">
                {message}
              </td>
              <td style="padding: 10px 12px; border-bottom: 1px solid #e2e8f0; font-size: 13px; text-align: center;">
                <span style="background-color: #f1f5f9; color: #475569; padding: 2px 8px; border-radius: 4px; font-family: monospace;">{fix_version}</span>
              </td>
              <td style="padding: 10px 12px; border-bottom: 1px solid #e2e8f0; font-size: 13px; text-align: center;">
                <span style="background-color: #e0f2fe; color: #0369a1; padding: 2px 8px; border-radius: 4px;">{status}</span>
              </td>
              <td style="padding: 10px 12px; border-bottom: 1px solid #e2e8f0; font-size: 13px;">
                👤 {author}
              </td>
              <td style="padding: 10px 12px; border-bottom: 1px solid #e2e8f0; font-size: 13px;">
                📋 {reporter}
              </td>
            </tr>
            "#
    )
}

fn section_html(title: &str, icon: &str, rows: &[AnnouncementRow], header_bg: &str) -> String {
    let count = rows.len();
    let body = if rows.is_empty() {
        r#"<tr><td colspan="6" style="padding: 16px; text-align: center; color: #94a3b8; font-style: italic;">No features in this section</td></tr>"#.to_owned()
    } else {
        rows.iter().map(row_html).collect()
    };
    format!(
        r#"
            <div style="margin-bottom: 30px; background: #ffffff; border-radius: 8px; border: 1px solid #e2e8f0; overflow: hidden; box-shadow: 0 1px 3px rgba(0,0,0,0.05);">
              <div style="background-color: {header_bg}; padding: 12px 18px; border-bottom: 1px solid #cbd5e1;">
                <h3 style="margin: 0; color: #0f172a; font-size: 16px; font-weight: 700;">
                  {icon} {title} ({count})
                </h3>
              </div>
              <table style="width: 100%; border-collapse: collapse; text-align: left; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;">
                <thead>
                  <tr style="background-color: #f8fafc; color: #64748b; font-size: 12px; text-transform: uppercase; letter-spacing: 0.05em;">
                    <th style="padding: 10px 12px; border-bottom: 1px solid #e2e8f0;">Ticket</th>
                    <th style="padding: 10px 12px; border-bottom: 1px solid #e2e8f0;">Summary / Commit</th>
                    <th style="padding: 10px 12px; border-bottom: 1px solid #e2e8f0; text-align: center;">Fix Version</th>
                    <th style="padding: 10px 12px; border-bottom: 1px solid #e2e8f0; text-align: center;">Status</th>
                    <th style="padding: 10px 12px; border-bottom: 1px solid #e2e8f0;">Committed By</th>
                    <th style="padding: 10px 12px; border-bottom: 1px solid #e2e8f0;">Jira Reporter</th>
                  </tr>
                </thead>
                <tbody>
                  {body}
                </tbody>
              </table>
            </div>
            "#
    )
}

fn html(
    fix_version: &str,
    branch: &str,
    included: &[AnnouncementRow],
    excluded: &[AnnouncementRow],
) -> String {
    let included_html = section_html("Included Features", "✅", included, "#f0fdf4");
    let excluded_html = section_html("Excluded Features (Filtered)", "🚫", excluded, "#fef2f2");
    let fv_text = escape(or(fix_version, "N/A"));
    let branch_text = escape(or(branch, "N/A"));
    let total = included.len() + excluded.len();
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Release Candidate Information - {fv_text}</title>
</head>
<body style="margin: 0; padding: 20px; background-color: #f1f5f9; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; color: #334155;">
  <div style="max-width: 960px; margin: 0 auto;">
{pad}
    <!-- Banner -->
    <div style="background: linear-gradient(135deg, #1e293b 0%, #0f172a 100%); color: #ffffff; padding: 24px; border-radius: 8px; margin-bottom: 24px; box-shadow: 0 4px 6px -1px rgba(0,0,0,0.1);">
      <h1 style="margin: 0 0 8px 0; font-size: 24px; font-weight: 700;">🚀 Release Candidate Overview</h1>
      <div style="font-size: 14px; color: #94a3b8;">
        <strong>Fix Version:</strong> <span style="color: #38bdf8;">{fv_text}</span> &nbsp;|&nbsp;
        <strong>Release Branch:</strong> <span style="color: #38bdf8;">{branch_text}</span> &nbsp;|&nbsp;
        <strong>Total Tickets:</strong> {total}
      </div>
    </div>

    <!-- Features Sections -->
    {included_html}
    {excluded_html}

    <!-- Footer -->
    <div style="text-align: center; color: #94a3b8; font-size: 12px; margin-top: 30px; padding-top: 20px; border-top: 1px solid #cbd5e1;">
      Sent via Release Manager UI (dbt-vdp)
    </div>

  </div>
</body>
</html>"#,
        // Python's template has a whitespace-only line; editors strip trailing blanks from literals.
        pad = "    "
    )
}

fn text(
    fix_version: &str,
    branch: &str,
    included: &[AnnouncementRow],
    excluded: &[AnnouncementRow],
) -> String {
    let mut out = format!(
        "RELEASE CANDIDATE OVERVIEW - {fix_version}\nBranch: {branch}\nTotal Merged Features: {}\n{}\n\nINCLUDED FEATURES ({}):",
        included.len() + excluded.len(),
        "=".repeat(60),
        included.len()
    );
    let line = |out: &mut String, row: &AnnouncementRow| {
        let _ = write!(
            out,
            "\n- {} | Status: {} | FixVersion: {} | Committed by: {} | Reporter: {}\n  Summary: {}",
            row.id, row.status, row.fix_version, row.committed_by, row.reporter, row.summary
        );
    };
    for row in included {
        line(&mut out, row);
    }
    let _ = write!(out, "\n\nEXCLUDED FEATURES ({}):", excluded.len());
    for row in excluded {
        line(&mut out, row);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_like_python() {
        assert_eq!(
            escape(r#"<a href="x">Tom & Jerry's</a>"#),
            "&lt;a href=&quot;x&quot;&gt;Tom &amp; Jerry&#x27;s&lt;/a&gt;"
        );
    }

    #[test]
    fn subject_names_fix_version_and_branch() {
        let a = Announcement::render("2026_DROP25", "release/2026_DROP25", vec![], vec![]);
        assert_eq!(
            a.subject,
            "🚀 Release Candidate Overview: 2026_DROP25 (release/2026_DROP25)"
        );
        assert_eq!(
            Announcement::render("", "", vec![], vec![]).subject,
            "🚀 Release Candidate Overview: N/A"
        );
        assert!(a.html.contains("No features in this section"));
    }
}
