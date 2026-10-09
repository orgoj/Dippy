//! Port of `src/dippy/cli/git.py`: approve read-only git operations.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["git"];
pub const PORTED: bool = true;
/// Module-level `get_description` (called with `include_context=False`).
pub const DESCRIPTION: Option<Describe> = Some(get_description);

/// Actions that only read data (`SAFE_ACTIONS`).
const SAFE_ACTIONS: &[&str] = &[
    "status",
    "log",
    "show",
    "diff",
    "blame",
    "annotate",
    "shortlog",
    "describe",
    "rev-parse",
    "rev-list",
    "reflog",
    "whatchanged",
    "diff-tree",
    "diff-files",
    "diff-index",
    "range-diff",
    "format-patch",
    "difftool",
    "grep",
    "ls-files",
    "ls-tree",
    "ls-remote",
    "cat-file",
    "verify-commit",
    "verify-tag",
    "name-rev",
    "merge-base",
    "show-ref",
    "show-branch",
    "check-ignore",
    "cherry",
    "for-each-ref",
    "count-objects",
    "fsck",
    "var",
    "request-pull",
    "archive",
    "fetch",
];

/// `UNCLEAR_ACTION_CONTEXT`.
fn unclear_action_context(action: &str) -> Option<&'static str> {
    match action {
        "gc" => Some("garbage collect"),
        "prune" => Some("remove unreachable objects"),
        "filter-branch" | "filter-repo" => Some("rewrite history"),
        _ => None,
    }
}

/// Git global flags that take an argument (`GLOBAL_FLAGS_WITH_ARG`).
const GLOBAL_FLAGS_WITH_ARG: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--super-prefix",
    "--config-env",
];

/// Git global flags without an argument (`GLOBAL_FLAGS_NO_ARG`).
const GLOBAL_FLAGS_NO_ARG: &[&str] = &[
    "--no-pager",
    "--paginate",
    "-p",
    "--no-replace-objects",
    "--bare",
    "--literal-pathspecs",
    "--glob-pathspecs",
    "--noglob-pathspecs",
    "--icase-pathspecs",
    "--no-optional-locks",
];

/// Find the git action (subcommand) accounting for global flags.
fn find_action(tokens: &[String]) -> Option<(usize, &str)> {
    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if GLOBAL_FLAGS_WITH_ARG.contains(&token) {
            i += 2;
            continue;
        }
        if GLOBAL_FLAGS_WITH_ARG.iter().any(|flag| {
            token
                .strip_prefix(flag)
                .is_some_and(|rest| rest.starts_with('='))
        }) {
            i += 1;
            continue;
        }
        if GLOBAL_FLAGS_NO_ARG.contains(&token) {
            i += 1;
            continue;
        }
        // (`-c key=value` is already covered by GLOBAL_FLAGS_WITH_ARG.)
        if !token.starts_with('-') {
            return Some((i, token));
        }
        break;
    }
    None
}

/// `get_description(tokens, include_context)`.
fn describe(tokens: &[String], include_context: bool) -> String {
    match find_action(tokens) {
        Some((_, action)) if !action.is_empty() => {
            if include_context && let Some(context) = unclear_action_context(action) {
                return format!("git {action} ({context})");
            }
            format!("git {action}")
        }
        _ => "git".into(),
    }
}

/// Module-level `get_description(tokens)`.
pub fn get_description(tokens: &[String]) -> String {
    describe(tokens, false)
}

pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() < 2 {
        return Classification::ask_desc("git");
    }
    let Some((action_idx, action)) = find_action(tokens) else {
        return Classification::ask_desc("git");
    };
    let rest: &[String] = tokens.get(action_idx + 1..).unwrap_or(&[]);

    let safe = match action {
        "branch" => check_branch(rest),
        "tag" => check_tag(rest),
        "remote" => check_remote(rest),
        "stash" => check_stash(rest),
        "config" => check_config(rest),
        "notes" => check_notes(rest),
        "bisect" => check_bisect(rest),
        "worktree" => check_worktree(rest),
        "submodule" => check_submodule(rest),
        "apply" => check_apply(rest),
        "sparse-checkout" => check_sparse_checkout(rest),
        "bundle" => check_bundle(rest),
        "lfs" => check_lfs(rest),
        "hash-object" => check_hash_object(rest),
        "symbolic-ref" => check_symbolic_ref(rest),
        "replace" => check_replace(rest),
        "rerere" => check_rerere(rest),
        other => SAFE_ACTIONS.contains(&other),
    };

    let desc = describe(tokens, !safe);
    if safe {
        Classification::allow_desc(desc)
    } else {
        Classification::ask_desc(desc)
    }
}

fn has(rest: &[String], value: &str) -> bool {
    rest.iter().any(|t| t == value)
}

fn first(rest: &[String]) -> Option<&str> {
    rest.first().map(String::as_str)
}

fn check_branch(rest: &[String]) -> bool {
    const UNSAFE_FLAGS: &[&str] = &[
        "-d", "-D", "--delete", "-m", "-M", "--move", "-c", "-C", "--copy",
    ];
    const LISTING_FLAGS_WITH_ARG: &[&str] = &[
        "--list",
        "-l",
        "--contains",
        "--no-contains",
        "--merged",
        "--no-merged",
        "--points-at",
    ];
    for token in rest {
        if UNSAFE_FLAGS.contains(&token.as_str()) {
            return false;
        }
        if token.starts_with("--set-upstream-to") || token == "-u" {
            return false;
        }
    }
    if rest
        .iter()
        .any(|t| LISTING_FLAGS_WITH_ARG.contains(&t.as_str()) || t.starts_with("--list"))
    {
        return true;
    }
    rest.iter().all(|t| t.starts_with('-'))
}

fn check_tag(rest: &[String]) -> bool {
    const UNSAFE_FLAGS: &[&str] = &["-d", "--delete"];
    const LISTING_FLAGS: &[&str] = &[
        "-l",
        "--list",
        "--contains",
        "--no-contains",
        "--merged",
        "--no-merged",
        "--points-at",
    ];
    if rest.iter().any(|t| UNSAFE_FLAGS.contains(&t.as_str())) {
        return false;
    }
    if rest
        .iter()
        .any(|t| LISTING_FLAGS.contains(&t.as_str()) || t.starts_with("--list"))
    {
        return true;
    }
    rest.iter().all(|t| t.starts_with('-'))
}

fn check_remote(rest: &[String]) -> bool {
    let Some(subcommand) = first(rest) else {
        return true;
    };
    if matches!(subcommand, "show" | "-v" | "--verbose" | "get-url") {
        return true;
    }
    if matches!(
        subcommand,
        "add" | "remove" | "rm" | "rename" | "set-url" | "prune" | "set-head" | "set-branches"
    ) {
        return false;
    }
    true
}

fn check_stash(rest: &[String]) -> bool {
    let Some(subcommand) = first(rest) else {
        return false;
    };
    matches!(subcommand, "list" | "show")
}

fn check_config(rest: &[String]) -> bool {
    const EDIT_FLAGS: &[&str] = &["-e", "--edit"];
    const UNSAFE_FLAGS: &[&str] = &[
        "--unset",
        "--unset-all",
        "--add",
        "--replace-all",
        "--remove-section",
        "--rename-section",
    ];
    const SAFE_FLAGS: &[&str] = &[
        "--get",
        "--get-all",
        "--list",
        "-l",
        "--get-regexp",
        "--get-urlmatch",
    ];
    const SCOPE_FLAGS: &[&str] = &["--global", "--local", "--system", "--worktree"];
    if rest
        .iter()
        .any(|t| EDIT_FLAGS.contains(&t.as_str()) || UNSAFE_FLAGS.contains(&t.as_str()))
    {
        return false;
    }
    if rest.iter().any(|t| SAFE_FLAGS.contains(&t.as_str())) {
        return true;
    }
    let actual_positional = rest
        .iter()
        .filter(|t| !t.starts_with('-') || SCOPE_FLAGS.contains(&t.as_str()))
        .filter(|t| !SCOPE_FLAGS.contains(&t.as_str()))
        .count();
    actual_positional <= 1
}

fn check_notes(rest: &[String]) -> bool {
    let Some(subcommand) = first(rest) else {
        return true;
    };
    if matches!(subcommand, "list" | "show") {
        return true;
    }
    !matches!(
        subcommand,
        "add" | "copy" | "append" | "edit" | "merge" | "remove" | "prune"
    )
}

fn check_bisect(rest: &[String]) -> bool {
    first(rest).is_some_and(|s| matches!(s, "log" | "visualize" | "view"))
}

fn check_worktree(rest: &[String]) -> bool {
    first(rest) == Some("list")
}

fn check_submodule(rest: &[String]) -> bool {
    first(rest).is_some_and(|s| matches!(s, "status" | "summary" | "foreach"))
}

fn check_apply(rest: &[String]) -> bool {
    has(rest, "--check")
}

fn check_sparse_checkout(rest: &[String]) -> bool {
    first(rest) == Some("list")
}

fn check_bundle(rest: &[String]) -> bool {
    first(rest).is_some_and(|s| matches!(s, "verify" | "list-heads"))
}

fn check_lfs(rest: &[String]) -> bool {
    first(rest).is_some_and(|s| matches!(s, "fetch" | "ls-files" | "status" | "env" | "version"))
}

fn check_hash_object(rest: &[String]) -> bool {
    !has(rest, "-w") && !has(rest, "--write")
}

fn check_symbolic_ref(rest: &[String]) -> bool {
    rest.iter().filter(|t| !t.starts_with('-')).count() <= 1
}

fn check_replace(rest: &[String]) -> bool {
    has(rest, "-l") || has(rest, "--list") || rest.is_empty()
}

fn check_rerere(rest: &[String]) -> bool {
    let Some(subcommand) = first(rest) else {
        return true;
    };
    matches!(subcommand, "status" | "diff")
}

#[cfg(test)]
mod tests {
    use super::super::Action;
    use super::*;

    fn run(cmd: &str) -> Classification {
        let tokens: Vec<&str> = cmd.split(' ').collect();
        classify(&HandlerContext::new(&tokens))
    }

    fn toks(cmd: &str) -> Vec<String> {
        cmd.split(' ').map(str::to_string).collect()
    }

    #[test]
    fn read_only_allows() {
        for cmd in [
            "git status",
            "git log --oneline -5",
            "git -C /tmp status",
            "git --git-dir=/x/.git log",
            "git -c core.pager=cat diff",
            "git --no-pager show HEAD",
            "git branch",
            "git branch -a",
            "git branch --list foo*",
            "git branch --contains abc",
            "git tag",
            "git tag -l v1*",
            "git remote",
            "git remote -v",
            "git remote origin",
            "git stash list",
            "git config --get user.name",
            "git config user.name",
            "git config --global user.name",
            "git notes",
            "git notes foo",
            "git bisect log",
            "git worktree list",
            "git submodule status",
            "git apply --check p.diff",
            "git sparse-checkout list",
            "git bundle verify b",
            "git lfs ls-files",
            "git hash-object f",
            "git symbolic-ref HEAD",
            "git replace",
            "git replace -l",
            "git rerere",
            "git rerere diff",
            "git fetch origin",
        ] {
            assert_eq!(run(cmd).action, Action::Allow, "{cmd}");
        }
    }

    #[test]
    fn mutations_ask() {
        for cmd in [
            "git",
            "git -C",
            "git --unknown status",
            "git commit -m x",
            "git push",
            "git branch new",
            "git branch -D old",
            "git branch -u origin/main",
            "git branch --set-upstream-to=origin/main",
            "git tag v1",
            "git tag -d v1",
            "git remote add o url",
            "git stash",
            "git stash pop",
            "git stash -u",
            "git config user.name x",
            "git config --unset user.name",
            "git config -e",
            "git notes add",
            "git bisect",
            "git bisect start",
            "git worktree",
            "git worktree add x",
            "git submodule update",
            "git apply p.diff",
            "git sparse-checkout set x",
            "git bundle create b",
            "git lfs pull",
            "git hash-object -w f",
            "git symbolic-ref HEAD refs/heads/x",
            "git replace a b",
            "git rerere forget x",
            "git gc",
        ] {
            assert_eq!(run(cmd).action, Action::Ask, "{cmd}");
        }
    }

    #[test]
    fn descriptions() {
        assert_eq!(
            run("git -C /x status").description.as_deref(),
            Some("git status")
        );
        assert_eq!(
            run("git gc --aggressive").description.as_deref(),
            Some("git gc (garbage collect)")
        );
        assert_eq!(
            run("git filter-branch").description.as_deref(),
            Some("git filter-branch (rewrite history)")
        );
        assert_eq!(run("git -x").description.as_deref(), Some("git"));
        assert_eq!(get_description(&toks("git gc")), "git gc");
        assert_eq!(get_description(&toks("git --bare")), "git");
        assert_eq!(get_description(&toks("git -C")), "git");
    }

    #[test]
    fn empty_action_token() {
        // `git ''`: action is "" (not None) -> ask with plain description.
        let c = classify(&HandlerContext::new(&["git", ""]));
        assert_eq!(c.action, Action::Ask);
        assert_eq!(c.description.as_deref(), Some("git"));
    }
}
