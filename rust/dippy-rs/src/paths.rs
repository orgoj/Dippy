//! Port of `dippy.core.paths` and the `os.path`/`pathlib` helpers it needs.

use std::path::{Component, Path, PathBuf};

/// `os.path.expanduser`: `~` and `~user` (unknown users stay literal).
pub fn expanduser(token: &str) -> String {
    if !token.starts_with('~') {
        return token.to_string();
    }
    let end = token.find('/').unwrap_or(token.len());
    let user = &token[1..end];
    let home = if user.is_empty() {
        match std::env::var("HOME") {
            Ok(h) => h,
            Err(_) => return token.to_string(),
        }
    } else {
        match user_home(user) {
            Some(h) => h,
            None => return token.to_string(),
        }
    };
    let home = if home.len() > 1 {
        home.trim_end_matches('/').to_string()
    } else {
        home
    };
    format!("{home}{}", &token[end..])
}

/// Home directory of a named user from /etc/passwd.
fn user_home(user: &str) -> Option<String> {
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        (fields.len() >= 6 && fields[0] == user).then(|| fields[5].to_string())
    })
}

/// `Path.home()`.
pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// `os.path.normpath` for POSIX paths (lexical).
pub fn normpath(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let initial = if path.starts_with("//") && !path.starts_with("///") {
        2
    } else if path.starts_with('/') {
        1
    } else {
        0
    };
    let mut comps: Vec<&str> = Vec::new();
    for comp in path.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp != ".." || (initial == 0 && comps.is_empty()) || comps.last() == Some(&"..") {
            comps.push(comp);
        } else if !comps.is_empty() {
            comps.pop();
        }
    }
    let joined = comps.join("/");
    let out = format!("{}{}", "/".repeat(initial), joined);
    if out.is_empty() { ".".into() } else { out }
}

/// `Path.resolve()` (non-strict): make absolute, resolve symlinks of the
/// existing prefix, normalise the rest lexically.
pub fn resolve(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let mut result = PathBuf::from("/");
    let mut seen = 0;
    for comp in abs.components() {
        match comp {
            Component::RootDir | Component::Prefix(_) => {}
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(name) => {
                let candidate = result.join(name);
                match std::fs::read_link(&candidate) {
                    Ok(target) if seen < 40 => {
                        seen += 1;
                        let base = result.clone();
                        let joined = if target.is_absolute() {
                            target
                        } else {
                            base.join(target)
                        };
                        result = resolve(&joined);
                    }
                    _ => result = candidate,
                }
            }
        }
    }
    result
}

/// Resolve a command-line path token for inspection (`resolve_arg_path`).
pub fn resolve_arg_path(token: &str, cwd: &Path) -> PathBuf {
    let path = PathBuf::from(expanduser(token));
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    resolve(&path)
}

/// `str(Path(x))`: Python's pathlib string form (collapses `//`, drops `.`).
pub fn pathlib_str(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let initial = if path.starts_with("//") && !path.starts_with("///") {
        "//"
    } else if path.starts_with('/') {
        "/"
    } else {
        ""
    };
    let parts: Vec<&str> = path
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let out = format!("{initial}{}", parts.join("/"));
    if out.is_empty() { ".".into() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normpath_cases() {
        assert_eq!(normpath("/a/b/../c"), "/a/c");
        assert_eq!(normpath("a/./b/"), "a/b");
        assert_eq!(normpath("../x"), "../x");
        assert_eq!(normpath("/.."), "/");
        assert_eq!(normpath("//a"), "//a");
        assert_eq!(normpath(""), ".");
    }

    #[test]
    fn pathlib_cases() {
        assert_eq!(pathlib_str("/a//b/./c/"), "/a/b/c");
        assert_eq!(pathlib_str("a/../b"), "a/../b");
    }
}
