//! Port of `src/dippy/cli/prometheus.py`.
//!
//! Prometheus is a monitoring server and time-series database. Only help and
//! version flags are safe (informational only). Running the server itself is
//! unsafe as it starts a service, binds ports, creates lockfiles, and writes
//! data to storage.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["prometheus"];
pub const PORTED: bool = true;
/// The Python module defines no `get_description`.
pub const DESCRIPTION: Option<Describe> = None;

/// Flags that are safe (informational only, don't start the server).
const SAFE_FLAGS: &[&str] = &["-h", "--help", "--help-long", "--help-man", "--version"];

/// Classify prometheus command.
///
/// Only help/version flags are safe. Any other invocation starts the server.
pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let base = tokens.first().map_or("prometheus", String::as_str);
    if tokens.len() < 2 {
        // Just "prometheus" with no args starts the server
        return Classification::ask_desc(format!("{base} server"));
    }

    // Prometheus doesn't have subcommands - it's all flags
    for token in &tokens[1..] {
        if SAFE_FLAGS.contains(&token.as_str()) {
            return Classification::allow_desc(format!("{base} {token}"));
        }
    }

    // Any other flags or arguments start the server
    Classification::ask_desc(format!("{base} server"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generated from the Python handler on the commands of
    /// `tests/cli/test_prometheus.py` (see `rust/parity/cases/prometheus.txt`).
    #[allow(clippy::type_complexity)]
    const PY_CASES: &[(&[&str], &[&str], &[bool], &str, Option<&str>)] = &[
        (
            &["prometheus", "--help"],
            &["prometheus", "--help"],
            &[false, false],
            "allow",
            Some("prometheus --help"),
        ),
        (
            &["prometheus", "-h"],
            &["prometheus", "-h"],
            &[false, false],
            "allow",
            Some("prometheus -h"),
        ),
        (
            &["prometheus", "--help-long"],
            &["prometheus", "--help-long"],
            &[false, false],
            "allow",
            Some("prometheus --help-long"),
        ),
        (
            &["prometheus", "--help-man"],
            &["prometheus", "--help-man"],
            &[false, false],
            "allow",
            Some("prometheus --help-man"),
        ),
        (
            &["prometheus", "--version"],
            &["prometheus", "--version"],
            &[false, false],
            "allow",
            Some("prometheus --version"),
        ),
        (
            &["prometheus"],
            &["prometheus"],
            &[false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--config.file=prometheus.yml"],
            &["prometheus", "--config.file=prometheus.yml"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--config.file=/etc/prometheus/prometheus.yml"],
            &["prometheus", "--config.file=/etc/prometheus/prometheus.yml"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--web.listen-address=0.0.0.0:9090"],
            &["prometheus", "--web.listen-address=0.0.0.0:9090"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--web.listen-address=:9090"],
            &["prometheus", "--web.listen-address=:9090"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--storage.tsdb.path=data/"],
            &["prometheus", "--storage.tsdb.path=data/"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--storage.tsdb.path=/var/lib/prometheus"],
            &["prometheus", "--storage.tsdb.path=/var/lib/prometheus"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--storage.tsdb.retention.time=15d"],
            &["prometheus", "--storage.tsdb.retention.time=15d"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--storage.tsdb.retention.size=512MB"],
            &["prometheus", "--storage.tsdb.retention.size=512MB"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--log.level=info"],
            &["prometheus", "--log.level=info"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--log.level=debug"],
            &["prometheus", "--log.level=debug"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--log.format=json"],
            &["prometheus", "--log.format=json"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--agent"],
            &["prometheus", "--agent"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--no-agent"],
            &["prometheus", "--no-agent"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--web.config.file=/etc/prometheus/web.yml"],
            &["prometheus", "--web.config.file=/etc/prometheus/web.yml"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &[
                "prometheus",
                "--web.external-url=http://prometheus.example.com",
            ],
            &[
                "prometheus",
                "--web.external-url=http://prometheus.example.com",
            ],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--web.route-prefix=/prometheus"],
            &["prometheus", "--web.route-prefix=/prometheus"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--web.enable-lifecycle"],
            &["prometheus", "--web.enable-lifecycle"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--web.enable-admin-api"],
            &["prometheus", "--web.enable-admin-api"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--web.enable-remote-write-receiver"],
            &["prometheus", "--web.enable-remote-write-receiver"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--web.enable-otlp-receiver"],
            &["prometheus", "--web.enable-otlp-receiver"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--web.console.templates=consoles"],
            &["prometheus", "--web.console.templates=consoles"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--web.console.libraries=console_libraries"],
            &["prometheus", "--web.console.libraries=console_libraries"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--storage.agent.path=data-agent/"],
            &["prometheus", "--storage.agent.path=data-agent/"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--storage.agent.wal-compression"],
            &["prometheus", "--storage.agent.wal-compression"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--storage.tsdb.no-lockfile"],
            &["prometheus", "--storage.tsdb.no-lockfile"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--storage.remote.flush-deadline=1m"],
            &["prometheus", "--storage.remote.flush-deadline=1m"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--query.timeout=2m"],
            &["prometheus", "--query.timeout=2m"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--query.max-concurrency=20"],
            &["prometheus", "--query.max-concurrency=20"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--query.max-samples=50000000"],
            &["prometheus", "--query.max-samples=50000000"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--rules.alert.for-outage-tolerance=1h"],
            &["prometheus", "--rules.alert.for-outage-tolerance=1h"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &[
                "prometheus",
                "--alertmanager.notification-queue-capacity=10000",
            ],
            &[
                "prometheus",
                "--alertmanager.notification-queue-capacity=10000",
            ],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--enable-feature=exemplar-storage"],
            &["prometheus", "--enable-feature=exemplar-storage"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--enable-feature=memory-snapshot-on-shutdown"],
            &["prometheus", "--enable-feature=memory-snapshot-on-shutdown"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--auto-gomaxprocs"],
            &["prometheus", "--auto-gomaxprocs"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &["prometheus", "--auto-gomemlimit"],
            &["prometheus", "--auto-gomemlimit"],
            &[false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &[
                "prometheus",
                "--config.file=prometheus.yml",
                "--storage.tsdb.path=/data",
                "--web.listen-address=:9090",
            ],
            &[
                "prometheus",
                "--config.file=prometheus.yml",
                "--storage.tsdb.path=/data",
                "--web.listen-address=:9090",
            ],
            &[false, false, false, false],
            "ask",
            Some("prometheus server"),
        ),
        (
            &[
                "prometheus",
                "--agent",
                "--storage.agent.path=/data",
                "--web.listen-address=:9090",
            ],
            &[
                "prometheus",
                "--agent",
                "--storage.agent.path=/data",
                "--web.listen-address=:9090",
            ],
            &[false, false, false, false],
            "ask",
            Some("prometheus server"),
        ),
    ];

    #[test]
    fn matches_python_handler() {
        for (tokens, raw, exp, action, desc) in PY_CASES {
            let mut ctx = HandlerContext::new(tokens);
            ctx.raw_words = raw.iter().map(|s| s.to_string()).collect();
            ctx.word_has_expansions = exp.to_vec();
            let result = classify(&ctx);
            assert_eq!(result.action.as_str(), *action, "{tokens:?}");
            assert_eq!(result.description.as_deref(), *desc, "{tokens:?}");
        }
    }
}
