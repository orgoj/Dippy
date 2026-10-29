//! systemctl and timedatectl (systemd).
//!
//! The read-only subcommands inspect units and the clock; every other
//! subcommand changes state (start, enable, set-time, ...) and asks.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["systemctl", "timedatectl"];
pub const PORTED: bool = true;
pub const DESCRIPTION: Option<Describe> = None;

const SYSTEMCTL_READS: &[&str] = &[
    "status",
    "show",
    "cat",
    "help",
    "is-active",
    "is-enabled",
    "is-failed",
    "is-system-running",
    "list-units",
    "list-unit-files",
    "list-timers",
    "list-sockets",
    "list-dependencies",
    "list-jobs",
    "list-paths",
    "list-automounts",
    "list-machines",
    "get-default",
];
/// Options whose value is the next word (`-p After`); `--opt=value` is one word.
const SYSTEMCTL_VALUE_OPTS: &[&str] = &[
    "-p",
    "-P",
    "-t",
    "-s",
    "-n",
    "-o",
    "-H",
    "-M",
    "--property",
    "--type",
    "--state",
    "--signal",
    "--lines",
    "--output",
    "--host",
    "--machine",
    "--root",
];
const TIMEDATECTL_READS: &[&str] = &[
    "status",
    "show",
    "help",
    "list-timezones",
    "timesync-status",
    "show-timesync",
];
const TIMEDATECTL_VALUE_OPTS: &[&str] = &["-p", "-H", "-M", "--property", "--host", "--machine"];

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let Some(cmd) = tokens.first() else {
        return Classification::ask();
    };
    let (reads, value_opts) = if cmd == "systemctl" {
        (SYSTEMCTL_READS, SYSTEMCTL_VALUE_OPTS)
    } else {
        (TIMEDATECTL_READS, TIMEDATECTL_VALUE_OPTS)
    };
    let mut args = tokens.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "-h" || arg == "--help" || arg == "--version" {
            return Classification::allow_desc(cmd.as_str());
        }
        if value_opts.contains(&arg.as_str()) {
            args.next();
        } else if !arg.starts_with('-') {
            let desc = format!("{cmd} {arg}");
            return if reads.contains(&arg.as_str()) {
                Classification::allow_desc(desc)
            } else {
                Classification::ask_desc(desc)
            };
        }
    }
    // Without a subcommand both print status (systemctl: list-units).
    Classification::allow_desc(cmd.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;

    fn run(tokens: &[&str]) -> Classification {
        classify(&HandlerContext::new(tokens))
    }

    #[test]
    fn systemctl_reads_allow() {
        for cmd in [
            &["systemctl"][..],
            &["systemctl", "status", "klipper"],
            &["systemctl", "show", "klipper", "-p", "After"],
            &["systemctl", "cat", "klipper.service", "--no-pager"],
            &["systemctl", "is-active", "klipper", "moonraker"],
            &["systemctl", "is-enabled", "chrony-wait.service"],
            &["systemctl", "is-failed", "x"],
            &["systemctl", "list-units", "--all", "--no-pager"],
            &["systemctl", "list-unit-files", "--no-pager"],
            &["systemctl", "list-timers", "--all"],
            &["systemctl", "--user", "status", "x"],
            &["systemctl", "--no-pager", "-p", "ActiveState", "show", "x"],
            &["systemctl", "--property=After", "show", "x"],
            &["systemctl", "--version"],
        ] {
            assert_eq!(run(cmd).action, Action::Allow, "{cmd:?}");
        }
    }

    #[test]
    fn systemctl_changes_ask() {
        for cmd in [
            &["systemctl", "restart", "klipper"][..],
            &["systemctl", "stop", "klipper"],
            &["systemctl", "enable", "--now", "x"],
            &["systemctl", "edit", "x"],
            &["systemctl", "daemon-reload"],
            &["systemctl", "--user", "start", "x"],
            &["systemctl", "-p", "status", "stop", "x"],
            &["systemctl", "set-property", "x", "CPUQuota=1%"],
            &["systemctl", "poweroff"],
            &["systemctl", "show-environment"],
        ] {
            let c = run(cmd);
            assert_eq!(c.action, Action::Ask, "{cmd:?}");
        }
        assert_eq!(
            run(&["systemctl", "restart", "klipper"])
                .description
                .as_deref(),
            Some("systemctl restart")
        );
    }

    #[test]
    fn timedatectl_reads_allow_and_changes_ask() {
        for cmd in [
            &["timedatectl"][..],
            &["timedatectl", "status"],
            &[
                "timedatectl",
                "show",
                "-p",
                "NTPSynchronized",
                "-p",
                "Timezone",
            ],
            &["timedatectl", "list-timezones"],
            &["timedatectl", "timesync-status"],
            &["timedatectl", "show-timesync", "--all"],
        ] {
            assert_eq!(run(cmd).action, Action::Allow, "{cmd:?}");
        }
        for cmd in [
            &["timedatectl", "set-time", "2020-01-01"][..],
            &["timedatectl", "set-timezone", "UTC"],
            &["timedatectl", "set-ntp", "false"],
            &["timedatectl", "-p", "show", "set-ntp", "false"],
        ] {
            assert_eq!(run(cmd).action, Action::Ask, "{cmd:?}");
        }
    }
}
