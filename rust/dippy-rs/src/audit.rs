//! Port of `dippy.audit` and `handle_audit_subcommand`: read-only queries
//! over the audit log and its daily rotations. Dates must be `YYYY-MM-DD`
//! (Python's `date.fromisoformat` also takes other ISO forms).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde_json::Value;

use crate::paths;

const GREP_FIELDS: [&str; 6] = [
    "command",
    "cmd",
    "message",
    "file_path",
    "tool",
    "suggestion",
];
/// Filters of `dippy audit`; they combine with AND.
#[derive(Debug, Default, PartialEq, clap::Args)]
pub struct Query {
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub since: Option<String>,
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub until: Option<String>,
    /// Keep only this decision (repeatable)
    #[arg(long = "decision", value_name = "DECISION", value_parser = ["allow", "ask", "deny", "pass"])]
    pub decisions: Vec<String>,
    /// Drop allow decisions
    #[arg(long)]
    pub not_allow: bool,
    /// claude, codex, agy, ...
    #[arg(long, value_name = "CLI")]
    pub agent: Option<String>,
    /// cwd at or below PATH
    #[arg(long, value_name = "PATH")]
    pub cwd: Option<String>,
    /// policy_cwd at or below PATH
    #[arg(long, value_name = "PATH")]
    pub policy_cwd: Option<String>,
    /// Agent tool name
    #[arg(long, value_name = "NAME")]
    pub tool: Option<String>,
    /// Substring of command, cmd, message or path
    #[arg(long, value_name = "TEXT")]
    pub grep: Option<String>,
    /// Count entries by field, e.g. cmd, command, cwd, tool (repeatable)
    #[arg(long, value_name = "FIELD")]
    pub group_by: Vec<String>,
    /// Last N entries or top N groups
    #[arg(long, value_name = "N", allow_negative_numbers = true)]
    pub limit: Option<i64>,
}

fn parse_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .filter(|_| value.len() == 10)
        .ok_or_else(|| format!("invalid date (expected YYYY-MM-DD): {value}"))
}

/// Rotated logs (oldest first) followed by the current log.
fn log_files(log: &Path, since: Option<NaiveDate>) -> Result<Vec<PathBuf>, String> {
    let dir = log
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut rotated = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(day) = name
                .strip_prefix("audit-")
                .and_then(|n| n.strip_suffix(".log"))
                .filter(|d| is_iso_shape(d))
            else {
                continue;
            };
            let path = dir.join(&name);
            if path == log {
                continue;
            }
            // A rotation named D holds entries written up to the morning of D+1.
            if let Some(since) = since
                && parse_date(day)? < since - chrono::Days::new(1)
            {
                continue;
            }
            rotated.push(path);
        }
    }
    rotated.sort();
    if log.is_file() {
        rotated.push(log.to_path_buf());
    }
    Ok(rotated)
}

/// `\d{4}-\d{2}-\d{2}` (Python's `\d` also matches non-ASCII digits).
fn is_iso_shape(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    chars.len() == 10
        && chars.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == '-',
            _ => c.is_numeric(),
        })
}

/// Python `str(value)` for JSON values (containers approximate `repr`).
fn py_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        other => other.to_string(),
    }
}

/// `_field`: `-` when missing, lists joined with commas.
fn field(entry: &serde_json::Map<String, Value>, name: &str) -> String {
    match entry.get(name) {
        None | Some(Value::Null) => "-".into(),
        Some(Value::Array(items)) => items.iter().map(py_str).collect::<Vec<_>>().join(","),
        Some(v) => py_str(v),
    }
}

/// `_under`: the value is the path or below it.
fn under(value: Option<&Value>, prefix: &str, cwd: &Path) -> bool {
    let Some(Value::String(value)) = value else {
        return false;
    };
    let expanded = paths::expanduser(prefix);
    let absolute = if expanded.starts_with('/') {
        expanded
    } else {
        format!("{}/{expanded}", cwd.display())
    };
    let prefix = paths::normpath(&absolute);
    let prefix = prefix.trim_end_matches('/');
    value == prefix || value.starts_with(&format!("{prefix}/"))
}

/// `query_audit_log`: raw JSON lines, or `count<TAB>values` when grouped.
/// `cwd` resolves relative `--cwd`/`--policy-cwd` filters.
pub fn query(log: &Path, q: &Query, cwd: &Path) -> Result<Vec<String>, String> {
    if let Some(limit) = q.limit.filter(|l| *l < 0) {
        return Err(format!("limit must not be negative: {limit}"));
    }
    let since_date = q.since.as_deref().map(parse_date).transpose()?;
    let until_date = q.until.as_deref().map(parse_date).transpose()?;
    let iso = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    let since = since_date.map(iso);
    let until_s = until_date.map(iso);

    let mut matches: Vec<(String, String, serde_json::Map<String, Value>)> = Vec::new();
    for path in log_files(log, since_date)? {
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        for line in String::from_utf8_lossy(&bytes).lines() {
            let line = line.trim();
            let Ok(Value::Object(entry)) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let ts = entry.get("ts").map(py_str).unwrap_or_default();
            let day: String = ts.chars().take(10).collect();
            if since.as_ref().is_some_and(|s| day < *s)
                || until_s.as_ref().is_some_and(|u| day > *u)
            {
                continue;
            }
            let decision = entry.get("decision").and_then(Value::as_str);
            if !q.decisions.is_empty()
                && !decision.is_some_and(|d| q.decisions.iter().any(|x| x == d))
            {
                continue;
            }
            if q.not_allow && decision == Some("allow") {
                continue;
            }
            let equals = |key: &str, want: &Option<String>| {
                want.as_ref()
                    .is_none_or(|w| entry.get(key).and_then(Value::as_str) == Some(w.as_str()))
            };
            if !equals("agent", &q.agent) || !equals("tool", &q.tool) {
                continue;
            }
            if q.cwd
                .as_ref()
                .is_some_and(|p| !under(entry.get("cwd"), p, cwd))
                || q.policy_cwd
                    .as_ref()
                    .is_some_and(|p| !under(entry.get("policy_cwd"), p, cwd))
            {
                continue;
            }
            if let Some(grep) = &q.grep
                && !GREP_FIELDS.iter().any(|name| {
                    entry
                        .get(*name)
                        .map(py_str)
                        .unwrap_or_default()
                        .contains(grep.as_str())
                })
            {
                continue;
            }
            matches.push((ts, line.to_string(), entry));
        }
    }
    matches.sort_by(|a, b| a.0.cmp(&b.0));
    let limit = q.limit.map(|l| l as usize);

    if !q.group_by.is_empty() {
        let mut counts: HashMap<Vec<String>, usize> = HashMap::new();
        for (_, _, entry) in &matches {
            let key = q.group_by.iter().map(|name| field(entry, name)).collect();
            *counts.entry(key).or_default() += 1;
        }
        let mut groups: Vec<(Vec<String>, usize)> = counts.into_iter().collect();
        groups.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        if let Some(limit) = limit {
            groups.truncate(limit);
        }
        return Ok(groups
            .into_iter()
            .map(|(key, count)| format!("{count}\t{}", key.join("\t")))
            .collect());
    }
    let lines: Vec<String> = matches.into_iter().map(|(_, line, _)| line).collect();
    let skip = limit.map_or(0, |l| lines.len().saturating_sub(l));
    Ok(lines.into_iter().skip(skip).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dippy-rs-audit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn filters_rotations_groups_and_limits() {
        let dir = temp_dir();
        let log = dir.join("audit.log");
        std::fs::write(
            dir.join("audit-2026-10-01.log"),
            "{\"decision\": \"ask\", \"cmd\": \"rm\", \"agent\": \"pi\", \"ts\": \"2026-10-01T09:00:00+00:00\"}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("audit-2026-10-08.log"),
            "{\"decision\": \"allow\", \"cmd\": \"ls\", \"cwd\": \"/w/sub\", \"ts\": \"2026-10-08T23:00:00+00:00\"}\n\
             not json\n[1]\n",
        )
        .unwrap();
        std::fs::write(
            &log,
            "{\"decision\": \"ask\", \"cmd\": \"rm\", \"cwd\": \"/w\", \"context_flags\": [\"a\", \"b\"], \"ts\": \"2026-10-09T08:00:00+00:00\"}\n\
             {\"decision\": \"deny\", \"cmd\": \"dd\", \"cwd\": \"/wx\", \"ts\": \"2026-10-09T07:00:00+00:00\"}\n",
        )
        .unwrap();
        let run = |q: Query| query(&log, &q, Path::new("/")).unwrap();
        let all = run(Query::default());
        assert_eq!(all.len(), 4);
        assert!(all[0].contains("2026-10-01") && all[3].contains("08:00"));
        let since = run(Query {
            since: Some("2026-10-09".into()),
            ..Query::default()
        });
        assert_eq!(since.len(), 2);
        assert!(since[0].contains("\"dd\""));
        assert_eq!(
            run(Query {
                cwd: Some("/w/".into()),
                not_allow: true,
                ..Query::default()
            })
            .len(),
            1
        );
        assert_eq!(
            run(Query {
                group_by: vec!["cmd".into(), "context_flags".into()],
                ..Query::default()
            }),
            ["1\tdd\t-", "1\tls\t-", "1\trm\t-", "1\trm\ta,b"]
        );
        assert_eq!(
            run(Query {
                group_by: vec!["cmd".into()],
                limit: Some(1),
                ..Query::default()
            }),
            ["2\trm"]
        );
        let last = run(Query {
            limit: Some(1),
            grep: Some("r".into()),
            ..Query::default()
        });
        assert_eq!(last.len(), 1);
        assert!(last[0].contains("08:00"));
        assert_eq!(
            query(
                &log,
                &Query {
                    limit: Some(-1),
                    ..Query::default()
                },
                Path::new("/")
            ),
            Err("limit must not be negative: -1".into())
        );
        assert!(
            query(
                &log,
                &Query {
                    since: Some("10/09/2026".into()),
                    ..Query::default()
                },
                Path::new("/")
            )
            .unwrap_err()
            .starts_with("invalid date")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[derive(clap::Parser)]
    struct Audit {
        #[command(flatten)]
        query: Query,
    }

    fn parse_args(args: &[&str]) -> Result<Query, clap::Error> {
        use clap::Parser;
        let argv = std::iter::once("audit").chain(args.iter().copied());
        Audit::try_parse_from(argv).map(|a| a.query)
    }

    #[test]
    fn parses_audit_arguments() {
        let args = [
            "--decision",
            "ask",
            "--decision=deny",
            "--limit",
            "3",
            "--cwd",
            ".",
        ];
        let q = parse_args(&args).unwrap();
        assert_eq!(q.decisions, ["ask", "deny"]);
        assert_eq!(q.limit, Some(3));
        assert_eq!(q.cwd.as_deref(), Some("."));
        assert!(parse_args(&["--decision", "maybe"]).is_err());
        assert!(parse_args(&["--bogus"]).is_err());
    }
}
