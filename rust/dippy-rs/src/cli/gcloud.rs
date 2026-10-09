//! Port of `src/dippy/cli/gcloud.py`.
//!
//! Handles gcloud and gsutil commands.

use super::{Classification, Describe, HandlerContext};

pub const COMMANDS: &[&str] = &["gcloud", "gsutil"];
pub const PORTED: bool = true;
pub const DESCRIPTION: Option<Describe> = Some(get_description);

/// Safe action keywords - these are read-only operations.
const SAFE_ACTION_KEYWORDS: &[&str] = &[
    "describe",
    "list",
    "get",
    "show",
    "info",
    "status",
    "version",
    "get-credentials", // Just configures kubectl
    "list-tags",
    "list-grantable-roles",
    "read",           // logging read, app logs read, etc.
    "configurations", // gcloud topic configurations is help
];

/// Safe action prefixes.
const SAFE_ACTION_PREFIXES: &[&str] = &["list-", "describe-", "get-"];

/// Unsafe action keywords - these modify state.
const UNSAFE_ACTION_KEYWORDS: &[&str] = &[
    "create",
    "delete",
    "remove",
    "update",
    "set",
    "add",
    "patch",
    "start",
    "stop",
    "restart",
    "reset",
    "deploy",
    "undelete",
    "enable",
    "disable",
    "import",
    "export",
    "ssh",
    "scp",
    "login",
    "activate",
    "revoke",
    "configure-docker",
    "print-access-token", // Sensitive operation
];
// Note: "run" is NOT here because it's also a gcloud command group name

/// Unsafe action patterns - match anywhere in action.
const UNSAFE_ACTION_PATTERNS: &[&str] = &[
    "add-iam-policy-binding",
    "remove-iam-policy-binding",
    "set-iam-policy",
];

/// Safe commands in specific groups.
const CONFIG_SAFE_COMMANDS: &[&str] = &["list", "get", "configurations"];

const AUTH_SAFE_COMMANDS: &[&str] = &["list"];

const PROJECTS_SAFE_COMMANDS: &[&str] = &["list", "describe", "get-ancestors", "get-iam-policy"];
const PROJECTS_UNSAFE_COMMANDS: &[&str] = &["create", "delete", "undelete", "update"];

/// Flags that consume the next token.
const FLAGS_WITH_ARG: &[&str] = &[
    "--project",
    "--region",
    "--zone",
    "--format",
    "--filter",
    "--cluster",
    "--location",
    "--instance",
    "--secret",
    "--service",
    "--keyring",
    "--member",
    "--role",
];

/// Compute description for gcloud command.
pub fn get_description(tokens: &[String]) -> String {
    let base = tokens.first().map_or("gcloud", String::as_str);
    if tokens.len() < 2 {
        return base.to_string();
    }
    if base == "gsutil" {
        for token in &tokens[1..] {
            if !token.starts_with('-') {
                return format!("gsutil {token}");
            }
        }
        return "gsutil".to_string();
    }
    let parts = extract_parts(&tokens[1..]);
    if parts.is_empty() {
        return base.to_string();
    }
    format!("{base} {}", parts[..parts.len().min(3)].join(" "))
}

/// Classify gcloud command.
pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    if tokens.len() < 2 {
        let base = tokens.first().map_or("gcloud", String::as_str);
        return Classification::ask_desc(base);
    }

    let base = tokens[0].as_str();
    let desc = get_description(tokens);

    // Handle gsutil separately
    if base == "gsutil" {
        if check_gsutil(tokens) {
            return Classification::allow_desc(desc);
        }
        return Classification::ask_desc(desc);
    }

    let parts = extract_parts(&tokens[1..]);
    if parts.is_empty() {
        return Classification::ask_desc(base);
    }

    // Help is always safe
    if parts.contains(&"help") || tokens.iter().any(|t| t == "--help" || t == "-h") {
        return Classification::allow_desc(desc);
    }

    // gcloud version/info/topic are safe
    if matches!(parts[0], "version" | "info" | "topic") {
        return Classification::allow_desc(desc);
    }

    // Handle config group
    if parts[0] == "config" {
        if parts.len() > 1 {
            if parts[1] == "set" {
                return Classification::ask_desc(desc);
            }
            if parts[1] == "configurations" && parts.len() > 2 {
                if matches!(parts[2], "create" | "activate" | "delete") {
                    return Classification::ask_desc(desc);
                }
                return Classification::allow_desc(desc);
            }
            if CONFIG_SAFE_COMMANDS.contains(&parts[1]) {
                return Classification::allow_desc(desc);
            }
        }
        return Classification::allow_desc(desc);
    }

    // Handle auth group - most commands modify state
    if parts[0] == "auth" {
        if parts.len() > 1 && AUTH_SAFE_COMMANDS.contains(&parts[1]) {
            return Classification::allow_desc(desc);
        }
        return Classification::ask_desc(desc);
    }

    // Handle projects group
    if parts[0] == "projects" {
        if parts.len() > 1 {
            let action = parts[1];
            if PROJECTS_SAFE_COMMANDS.contains(&action) {
                return Classification::allow_desc(desc);
            }
            if PROJECTS_UNSAFE_COMMANDS.contains(&action) {
                return Classification::ask_desc(desc);
            }
            if action.contains("iam-policy-binding") || action.contains("iam-policy") {
                return Classification::ask_desc(desc);
            }
            return Classification::ask_desc(desc);
        }
        return Classification::allow_desc(desc);
    }

    // Skip beta/alpha prefix for action checking
    let action_parts: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|p| !matches!(*p, "beta" | "alpha"))
        .collect();

    // Check for unsafe patterns in any part (takes precedence)
    for part in &action_parts {
        if UNSAFE_ACTION_PATTERNS.iter().any(|p| part.contains(p)) {
            return Classification::ask_desc(desc);
        }
    }

    // Check ALL parts for unsafe keywords (takes precedence over safe)
    for part in &action_parts {
        if UNSAFE_ACTION_KEYWORDS.contains(part) {
            return Classification::ask_desc(desc);
        }
    }

    // Check all parts for safe keywords
    for part in &action_parts {
        if SAFE_ACTION_KEYWORDS.contains(part) {
            return Classification::allow_desc(desc);
        }
        if SAFE_ACTION_PREFIXES.iter().any(|p| part.starts_with(p)) {
            return Classification::allow_desc(desc);
        }
    }

    Classification::ask_desc(desc)
}

/// Extract command parts (service/subgroup/action), skipping flags and their
/// arguments.
fn extract_parts(tokens: &[String]) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut i = 0;

    while i < tokens.len() && parts.len() < 6 {
        let token = tokens[i].as_str();

        // Skip flags
        if token.starts_with('-') {
            if FLAGS_WITH_ARG.contains(&token) && i + 1 < tokens.len() {
                i += 2; // Skip flag and its argument
            } else {
                i += 1; // Flag with value like --format=json, or boolean flag
            }
            continue;
        }

        // Skip what looks like values (URIs, paths, emails)
        if looks_like_value(token) {
            i += 1;
            continue;
        }

        // This is a command part
        parts.push(token);
        i += 1;
    }

    parts
}

/// Check if token looks like a value rather than a command.
fn looks_like_value(token: &str) -> bool {
    // GCS paths
    token.starts_with("gs://")
        // GCR paths
        || token.starts_with("gcr.io/")
        // Resource paths
        || token.starts_with("//")
        // Email addresses
        || token.contains('@')
        // Numeric values
        || py_isdigit(token)
        // Single quotes (filter expressions)
        || token.starts_with('\'')
}

/// Code point ranges for which Python's `str.isdigit()` is true
/// (Numeric_Type Decimal or Digit), generated from Python 3.12.
const PY_DIGIT_RANGES: &[(u32, u32)] = &[
    (0x30, 0x39),
    (0xB2, 0xB3),
    (0xB9, 0xB9),
    (0x660, 0x669),
    (0x6F0, 0x6F9),
    (0x7C0, 0x7C9),
    (0x966, 0x96F),
    (0x9E6, 0x9EF),
    (0xA66, 0xA6F),
    (0xAE6, 0xAEF),
    (0xB66, 0xB6F),
    (0xBE6, 0xBEF),
    (0xC66, 0xC6F),
    (0xCE6, 0xCEF),
    (0xD66, 0xD6F),
    (0xDE6, 0xDEF),
    (0xE50, 0xE59),
    (0xED0, 0xED9),
    (0xF20, 0xF29),
    (0x1040, 0x1049),
    (0x1090, 0x1099),
    (0x1369, 0x1371),
    (0x17E0, 0x17E9),
    (0x1810, 0x1819),
    (0x1946, 0x194F),
    (0x19D0, 0x19DA),
    (0x1A80, 0x1A89),
    (0x1A90, 0x1A99),
    (0x1B50, 0x1B59),
    (0x1BB0, 0x1BB9),
    (0x1C40, 0x1C49),
    (0x1C50, 0x1C59),
    (0x2070, 0x2070),
    (0x2074, 0x2079),
    (0x2080, 0x2089),
    (0x2460, 0x2468),
    (0x2474, 0x247C),
    (0x2488, 0x2490),
    (0x24EA, 0x24EA),
    (0x24F5, 0x24FD),
    (0x24FF, 0x24FF),
    (0x2776, 0x277E),
    (0x2780, 0x2788),
    (0x278A, 0x2792),
    (0xA620, 0xA629),
    (0xA8D0, 0xA8D9),
    (0xA900, 0xA909),
    (0xA9D0, 0xA9D9),
    (0xA9F0, 0xA9F9),
    (0xAA50, 0xAA59),
    (0xABF0, 0xABF9),
    (0xFF10, 0xFF19),
    (0x104A0, 0x104A9),
    (0x10A40, 0x10A43),
    (0x10D30, 0x10D39),
    (0x10E60, 0x10E68),
    (0x11052, 0x1105A),
    (0x11066, 0x1106F),
    (0x110F0, 0x110F9),
    (0x11136, 0x1113F),
    (0x111D0, 0x111D9),
    (0x112F0, 0x112F9),
    (0x11450, 0x11459),
    (0x114D0, 0x114D9),
    (0x11650, 0x11659),
    (0x116C0, 0x116C9),
    (0x11730, 0x11739),
    (0x118E0, 0x118E9),
    (0x11950, 0x11959),
    (0x11C50, 0x11C59),
    (0x11D50, 0x11D59),
    (0x11DA0, 0x11DA9),
    (0x11F50, 0x11F59),
    (0x16A60, 0x16A69),
    (0x16AC0, 0x16AC9),
    (0x16B50, 0x16B59),
    (0x1D7CE, 0x1D7FF),
    (0x1E140, 0x1E149),
    (0x1E2F0, 0x1E2F9),
    (0x1E4F0, 0x1E4F9),
    (0x1E950, 0x1E959),
    (0x1F100, 0x1F10A),
    (0x1FBF0, 0x1FBF9),
];

/// Python `str.isdigit()`: non-empty and every character is a digit.
pub(crate) fn py_isdigit(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| {
            let cp = c as u32;
            PY_DIGIT_RANGES
                .iter()
                .any(|&(lo, hi)| (lo..=hi).contains(&cp))
        })
}

/// Check gsutil commands.
fn check_gsutil(tokens: &[String]) -> bool {
    if tokens.len() < 2 {
        return false;
    }

    let action = tokens[1..]
        .iter()
        .map(String::as_str)
        .find(|t| !t.starts_with('-'));

    // Python `if not action` (None or empty string)
    match action {
        Some(a) if !a.is_empty() => {
            matches!(
                a,
                "ls" | "cat" | "stat" | "du" | "hash" | "version" | "help"
            )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isdigit_matches_python() {
        assert!(py_isdigit("123"));
        assert!(py_isdigit("²"));
        assert!(py_isdigit("٣"));
        assert!(!py_isdigit(""));
        assert!(!py_isdigit("½"));
        assert!(!py_isdigit("Ⅻ"));
        assert!(!py_isdigit("12a"));
    }

    #[test]
    fn description_depth() {
        let t: Vec<String> = [
            "gcloud",
            "--project",
            "p",
            "compute",
            "instances",
            "list",
            "x",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(get_description(&t), "gcloud compute instances list");
    }

    /// Generated from the Python handler on the commands of
    /// `tests/cli/test_gcloud.py` (see `rust/parity/cases/gcloud.txt`).
    #[allow(clippy::type_complexity)]
    const PY_CASES: &[(&[&str], &[&str], &[bool], &str, Option<&str>)] = &[
        (
            &[
                "gcloud",
                "--project",
                "delete",
                "compute",
                "instances",
                "list",
            ],
            &[
                "gcloud",
                "--project",
                "delete",
                "compute",
                "instances",
                "list",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud compute instances list"),
        ),
        (
            &[
                "gcloud",
                "--format",
                "delete",
                "compute",
                "instances",
                "list",
            ],
            &[
                "gcloud",
                "--format",
                "delete",
                "compute",
                "instances",
                "list",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud compute instances list"),
        ),
        (
            &[
                "gcloud",
                "--project",
                "myproj",
                "compute",
                "instances",
                "delete",
                "foo",
            ],
            &[
                "gcloud",
                "--project",
                "myproj",
                "compute",
                "instances",
                "delete",
                "foo",
            ],
            &[false, false, false, false, false, false, false],
            "ask",
            Some("gcloud compute instances delete"),
        ),
        (
            &["gcloud", "compute", "instances", "list"],
            &["gcloud", "compute", "instances", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud compute instances list"),
        ),
        (
            &["gcloud", "compute", "instances", "list", "--project", "foo"],
            &["gcloud", "compute", "instances", "list", "--project", "foo"],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud compute instances list"),
        ),
        (
            &[
                "gcloud",
                "compute",
                "backend-services",
                "describe",
                "k8s-be",
                "--global",
                "--project",
                "foo",
            ],
            &[
                "gcloud",
                "compute",
                "backend-services",
                "describe",
                "k8s-be",
                "--global",
                "--project",
                "foo",
            ],
            &[false, false, false, false, false, false, false, false],
            "allow",
            Some("gcloud compute backend-services describe"),
        ),
        (
            &[
                "gcloud",
                "iap",
                "settings",
                "get",
                "--project",
                "foo",
                "--resource-type=compute",
            ],
            &[
                "gcloud",
                "iap",
                "settings",
                "get",
                "--project",
                "foo",
                "--resource-type=compute",
            ],
            &[false, false, false, false, false, false, false],
            "allow",
            Some("gcloud iap settings get"),
        ),
        (
            &["gcloud", "auth", "list"],
            &["gcloud", "auth", "list"],
            &[false, false, false],
            "allow",
            Some("gcloud auth list"),
        ),
        (
            &["gcloud", "compute", "instances", "delete", "foo"],
            &["gcloud", "compute", "instances", "delete", "foo"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute instances delete"),
        ),
        (
            &["gcloud", "compute", "instances", "delete", "list"],
            &["gcloud", "compute", "instances", "delete", "list"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute instances delete"),
        ),
        (
            &["gcloud", "compute", "instances", "create", "foo"],
            &["gcloud", "compute", "instances", "create", "foo"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute instances create"),
        ),
        (
            &["gcloud", "container", "clusters", "get-credentials", "foo"],
            &["gcloud", "container", "clusters", "get-credentials", "foo"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud container clusters get-credentials"),
        ),
        (
            &["gcloud", "run", "services", "list"],
            &["gcloud", "run", "services", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud run services list"),
        ),
        (
            &[
                "gcloud",
                "run",
                "services",
                "describe",
                "myservice",
                "--region",
                "us-central1",
            ],
            &[
                "gcloud",
                "run",
                "services",
                "describe",
                "myservice",
                "--region",
                "us-central1",
            ],
            &[false, false, false, false, false, false, false],
            "allow",
            Some("gcloud run services describe"),
        ),
        (
            &[
                "gcloud",
                "run",
                "services",
                "update",
                "myservice",
                "--region",
                "us-central1",
            ],
            &[
                "gcloud",
                "run",
                "services",
                "update",
                "myservice",
                "--region",
                "us-central1",
            ],
            &[false, false, false, false, false, false, false],
            "ask",
            Some("gcloud run services update"),
        ),
        (
            &["gcloud", "run", "services", "delete", "myservice"],
            &["gcloud", "run", "services", "delete", "myservice"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud run services delete"),
        ),
        (
            &[
                "gcloud",
                "compute",
                "backend-services",
                "list",
                "--project",
                "foo",
            ],
            &[
                "gcloud",
                "compute",
                "backend-services",
                "list",
                "--project",
                "foo",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud compute backend-services list"),
        ),
        (
            &[
                "gcloud",
                "compute",
                "ssl-certificates",
                "describe",
                "mycert",
                "--global",
            ],
            &[
                "gcloud",
                "compute",
                "ssl-certificates",
                "describe",
                "mycert",
                "--global",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud compute ssl-certificates describe"),
        ),
        (
            &[
                "gcloud",
                "iap",
                "web",
                "get-iam-policy",
                "--resource-type=backend-services",
            ],
            &[
                "gcloud",
                "iap",
                "web",
                "get-iam-policy",
                "--resource-type=backend-services",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud iap web get-iam-policy"),
        ),
        (
            &[
                "gcloud",
                "artifacts",
                "docker",
                "images",
                "list",
                "us-central1-docker.pkg.dev/proj/repo",
            ],
            &[
                "gcloud",
                "artifacts",
                "docker",
                "images",
                "list",
                "us-central1-docker.pkg.dev/proj/repo",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud artifacts docker images"),
        ),
        (
            &["gcloud", "iam", "service-accounts", "list"],
            &["gcloud", "iam", "service-accounts", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud iam service-accounts list"),
        ),
        (
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "delete",
                "sa@proj.iam.gserviceaccount.com",
            ],
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "delete",
                "sa@proj.iam.gserviceaccount.com",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud iam service-accounts delete"),
        ),
        (
            &["gcloud", "secrets", "list", "--project", "foo"],
            &["gcloud", "secrets", "list", "--project", "foo"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud secrets list"),
        ),
        (
            &["gcloud", "secrets", "describe", "mysecret"],
            &["gcloud", "secrets", "describe", "mysecret"],
            &[false, false, false, false],
            "allow",
            Some("gcloud secrets describe mysecret"),
        ),
        (
            &["gcloud", "secrets", "create", "newsecret"],
            &["gcloud", "secrets", "create", "newsecret"],
            &[false, false, false, false],
            "ask",
            Some("gcloud secrets create newsecret"),
        ),
        (
            &["gcloud", "dns", "record-sets", "list", "--zone", "myzone"],
            &["gcloud", "dns", "record-sets", "list", "--zone", "myzone"],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud dns record-sets list"),
        ),
        (
            &["gcloud", "functions", "list", "--project", "foo"],
            &["gcloud", "functions", "list", "--project", "foo"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud functions list"),
        ),
        (
            &["gcloud", "config", "get-value", "project"],
            &["gcloud", "config", "get-value", "project"],
            &[false, false, false, false],
            "allow",
            Some("gcloud config get-value project"),
        ),
        (
            &["gcloud", "config", "set", "project", "foo"],
            &["gcloud", "config", "set", "project", "foo"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud config set project"),
        ),
        (
            &[
                "gcloud",
                "logging",
                "read",
                "resource.type=cloud_run_revision",
            ],
            &[
                "gcloud",
                "logging",
                "read",
                "'resource.type=cloud_run_revision'",
            ],
            &[false, false, false, false],
            "allow",
            Some("gcloud logging read resource.type=cloud_run_revision"),
        ),
        (
            &["gcloud", "storage", "buckets", "describe", "gs://mybucket"],
            &["gcloud", "storage", "buckets", "describe", "gs://mybucket"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud storage buckets describe"),
        ),
        (
            &["gcloud", "beta", "run", "services", "describe", "myservice"],
            &["gcloud", "beta", "run", "services", "describe", "myservice"],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud beta run services"),
        ),
        (
            &["gcloud", "beta", "run", "services", "update", "myservice"],
            &["gcloud", "beta", "run", "services", "update", "myservice"],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud beta run services"),
        ),
        (
            &[
                "gcloud",
                "certificate-manager",
                "trust-configs",
                "describe",
                "myconfig",
            ],
            &[
                "gcloud",
                "certificate-manager",
                "trust-configs",
                "describe",
                "myconfig",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud certificate-manager trust-configs describe"),
        ),
        (
            &[
                "gcloud",
                "network-security",
                "server-tls-policies",
                "describe",
                "mypolicy",
            ],
            &[
                "gcloud",
                "network-security",
                "server-tls-policies",
                "describe",
                "mypolicy",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud network-security server-tls-policies describe"),
        ),
        (
            &[
                "gcloud",
                "container",
                "images",
                "list-tags",
                "gcr.io/proj/image",
            ],
            &[
                "gcloud",
                "container",
                "images",
                "list-tags",
                "gcr.io/proj/image",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud container images list-tags"),
        ),
        (
            &["gcloud", "projects", "list"],
            &["gcloud", "projects", "list"],
            &[false, false, false],
            "allow",
            Some("gcloud projects list"),
        ),
        (
            &["gcloud", "projects", "describe", "myproject"],
            &["gcloud", "projects", "describe", "myproject"],
            &[false, false, false, false],
            "allow",
            Some("gcloud projects describe myproject"),
        ),
        (
            &["gcloud", "projects", "get-iam-policy", "myproject"],
            &["gcloud", "projects", "get-iam-policy", "myproject"],
            &[false, false, false, false],
            "allow",
            Some("gcloud projects get-iam-policy myproject"),
        ),
        (
            &[
                "gcloud",
                "projects",
                "add-iam-policy-binding",
                "myproject",
                "--member=user:foo",
            ],
            &[
                "gcloud",
                "projects",
                "add-iam-policy-binding",
                "myproject",
                "--member=user:foo",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud projects add-iam-policy-binding myproject"),
        ),
        (
            &["gcloud", "config", "list"],
            &["gcloud", "config", "list"],
            &[false, false, false],
            "allow",
            Some("gcloud config list"),
        ),
        (
            &["gcloud", "config", "get", "project"],
            &["gcloud", "config", "get", "project"],
            &[false, false, false, false],
            "allow",
            Some("gcloud config get project"),
        ),
        (
            &["gcloud", "config", "get", "compute/zone"],
            &["gcloud", "config", "get", "compute/zone"],
            &[false, false, false, false],
            "allow",
            Some("gcloud config get compute/zone"),
        ),
        (
            &["gcloud", "config", "set", "project", "my-project"],
            &["gcloud", "config", "set", "project", "my-project"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud config set project"),
        ),
        (
            &["gcloud", "config", "set", "compute/zone", "us-central1-a"],
            &["gcloud", "config", "set", "compute/zone", "us-central1-a"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud config set compute/zone"),
        ),
        (
            &["gcloud", "config", "configurations", "list"],
            &["gcloud", "config", "configurations", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud config configurations list"),
        ),
        (
            &["gcloud", "config", "configurations", "create", "new-config"],
            &["gcloud", "config", "configurations", "create", "new-config"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud config configurations create"),
        ),
        (
            &[
                "gcloud",
                "config",
                "configurations",
                "activate",
                "new-config",
            ],
            &[
                "gcloud",
                "config",
                "configurations",
                "activate",
                "new-config",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud config configurations activate"),
        ),
        (
            &["gcloud", "auth", "login"],
            &["gcloud", "auth", "login"],
            &[false, false, false],
            "ask",
            Some("gcloud auth login"),
        ),
        (
            &["gcloud", "auth", "activate-service-account"],
            &["gcloud", "auth", "activate-service-account"],
            &[false, false, false],
            "ask",
            Some("gcloud auth activate-service-account"),
        ),
        (
            &["gcloud", "auth", "application-default", "login"],
            &["gcloud", "auth", "application-default", "login"],
            &[false, false, false, false],
            "ask",
            Some("gcloud auth application-default login"),
        ),
        (
            &["gcloud", "auth", "print-access-token"],
            &["gcloud", "auth", "print-access-token"],
            &[false, false, false],
            "ask",
            Some("gcloud auth print-access-token"),
        ),
        (
            &["gcloud", "auth", "revoke"],
            &["gcloud", "auth", "revoke"],
            &[false, false, false],
            "ask",
            Some("gcloud auth revoke"),
        ),
        (
            &["gcloud", "auth", "configure-docker"],
            &["gcloud", "auth", "configure-docker"],
            &[false, false, false],
            "ask",
            Some("gcloud auth configure-docker"),
        ),
        (
            &["gcloud", "components", "list"],
            &["gcloud", "components", "list"],
            &[false, false, false],
            "allow",
            Some("gcloud components list"),
        ),
        (
            &["gcloud", "components", "install", "kubectl"],
            &["gcloud", "components", "install", "kubectl"],
            &[false, false, false, false],
            "ask",
            Some("gcloud components install kubectl"),
        ),
        (
            &["gcloud", "components", "update"],
            &["gcloud", "components", "update"],
            &[false, false, false],
            "ask",
            Some("gcloud components update"),
        ),
        (
            &["gcloud", "components", "update", "--version=1.2.3"],
            &["gcloud", "components", "update", "--version=1.2.3"],
            &[false, false, false, false],
            "ask",
            Some("gcloud components update"),
        ),
        (
            &["gcloud", "components", "update", "--quiet"],
            &["gcloud", "components", "update", "--quiet"],
            &[false, false, false, false],
            "ask",
            Some("gcloud components update"),
        ),
        (
            &["gcloud", "compute", "zones", "list"],
            &["gcloud", "compute", "zones", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud compute zones list"),
        ),
        (
            &["gcloud", "compute", "instances", "create", "my-instance"],
            &["gcloud", "compute", "instances", "create", "my-instance"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute instances create"),
        ),
        (
            &["gcloud", "compute", "instances", "describe", "my-instance"],
            &["gcloud", "compute", "instances", "describe", "my-instance"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute instances describe"),
        ),
        (
            &[
                "gcloud",
                "compute",
                "instances",
                "list",
                "--filter='status=RUNNING'",
            ],
            &[
                "gcloud",
                "compute",
                "instances",
                "list",
                "--filter='status=RUNNING'",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute instances list"),
        ),
        (
            &["gcloud", "compute", "instances", "delete", "my-instance"],
            &["gcloud", "compute", "instances", "delete", "my-instance"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute instances delete"),
        ),
        (
            &["gcloud", "compute", "instances", "start", "my-instance"],
            &["gcloud", "compute", "instances", "start", "my-instance"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute instances start"),
        ),
        (
            &["gcloud", "compute", "instances", "stop", "my-instance"],
            &["gcloud", "compute", "instances", "stop", "my-instance"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute instances stop"),
        ),
        (
            &["gcloud", "compute", "disks", "list"],
            &["gcloud", "compute", "disks", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud compute disks list"),
        ),
        (
            &["gcloud", "compute", "disks", "describe", "my-disk"],
            &["gcloud", "compute", "disks", "describe", "my-disk"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute disks describe"),
        ),
        (
            &[
                "gcloud",
                "compute",
                "disks",
                "snapshot",
                "my-disk",
                "--snapshot-names=my-snapshot",
            ],
            &[
                "gcloud",
                "compute",
                "disks",
                "snapshot",
                "my-disk",
                "--snapshot-names=my-snapshot",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud compute disks snapshot"),
        ),
        (
            &["gcloud", "compute", "disks", "create", "my-disk"],
            &["gcloud", "compute", "disks", "create", "my-disk"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute disks create"),
        ),
        (
            &["gcloud", "compute", "disks", "delete", "my-disk"],
            &["gcloud", "compute", "disks", "delete", "my-disk"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute disks delete"),
        ),
        (
            &["gcloud", "compute", "snapshots", "list"],
            &["gcloud", "compute", "snapshots", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud compute snapshots list"),
        ),
        (
            &["gcloud", "compute", "snapshots", "describe", "my-snapshot"],
            &["gcloud", "compute", "snapshots", "describe", "my-snapshot"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute snapshots describe"),
        ),
        (
            &["gcloud", "compute", "snapshots", "delete", "my-snapshot"],
            &["gcloud", "compute", "snapshots", "delete", "my-snapshot"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute snapshots delete"),
        ),
        (
            &["gcloud", "compute", "ssh", "my-instance"],
            &["gcloud", "compute", "ssh", "my-instance"],
            &[false, false, false, false],
            "ask",
            Some("gcloud compute ssh my-instance"),
        ),
        (
            &[
                "gcloud",
                "compute",
                "ssh",
                "user@my-instance",
                "--zone=us-central1-a",
            ],
            &[
                "gcloud",
                "compute",
                "ssh",
                "user@my-instance",
                "--zone=us-central1-a",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute ssh"),
        ),
        (
            &["gcloud", "compute", "regions", "list"],
            &["gcloud", "compute", "regions", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud compute regions list"),
        ),
        (
            &["gcloud", "compute", "regions", "describe", "us-central1"],
            &["gcloud", "compute", "regions", "describe", "us-central1"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute regions describe"),
        ),
        (
            &["gcloud", "compute", "zones", "describe", "us-central1-a"],
            &["gcloud", "compute", "zones", "describe", "us-central1-a"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute zones describe"),
        ),
        (
            &["gcloud", "compute", "networks", "list"],
            &["gcloud", "compute", "networks", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud compute networks list"),
        ),
        (
            &["gcloud", "compute", "networks", "describe", "my-network"],
            &["gcloud", "compute", "networks", "describe", "my-network"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute networks describe"),
        ),
        (
            &["gcloud", "compute", "networks", "create", "my-network"],
            &["gcloud", "compute", "networks", "create", "my-network"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute networks create"),
        ),
        (
            &["gcloud", "compute", "networks", "delete", "my-network"],
            &["gcloud", "compute", "networks", "delete", "my-network"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute networks delete"),
        ),
        (
            &["gcloud", "compute", "firewall-rules", "list"],
            &["gcloud", "compute", "firewall-rules", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud compute firewall-rules list"),
        ),
        (
            &["gcloud", "compute", "firewall-rules", "describe", "my-rule"],
            &["gcloud", "compute", "firewall-rules", "describe", "my-rule"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute firewall-rules describe"),
        ),
        (
            &[
                "gcloud",
                "compute",
                "firewall-rules",
                "create",
                "my-rule",
                "--allow=tcp:22",
            ],
            &[
                "gcloud",
                "compute",
                "firewall-rules",
                "create",
                "my-rule",
                "--allow=tcp:22",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud compute firewall-rules create"),
        ),
        (
            &["gcloud", "compute", "firewall-rules", "delete", "my-rule"],
            &["gcloud", "compute", "firewall-rules", "delete", "my-rule"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud compute firewall-rules delete"),
        ),
        (
            &["gcloud", "container", "clusters", "list"],
            &["gcloud", "container", "clusters", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud container clusters list"),
        ),
        (
            &["gcloud", "container", "clusters", "describe", "my-cluster"],
            &["gcloud", "container", "clusters", "describe", "my-cluster"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud container clusters describe"),
        ),
        (
            &[
                "gcloud",
                "container",
                "clusters",
                "get-credentials",
                "my-cluster",
            ],
            &[
                "gcloud",
                "container",
                "clusters",
                "get-credentials",
                "my-cluster",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud container clusters get-credentials"),
        ),
        (
            &["gcloud", "container", "clusters", "create", "my-cluster"],
            &["gcloud", "container", "clusters", "create", "my-cluster"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud container clusters create"),
        ),
        (
            &["gcloud", "container", "clusters", "delete", "my-cluster"],
            &["gcloud", "container", "clusters", "delete", "my-cluster"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud container clusters delete"),
        ),
        (
            &["gcloud", "container", "clusters", "update", "my-cluster"],
            &["gcloud", "container", "clusters", "update", "my-cluster"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud container clusters update"),
        ),
        (
            &[
                "gcloud",
                "container",
                "clusters",
                "resize",
                "my-cluster",
                "--size=5",
            ],
            &[
                "gcloud",
                "container",
                "clusters",
                "resize",
                "my-cluster",
                "--size=5",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud container clusters resize"),
        ),
        (
            &["gcloud", "container", "images", "list"],
            &["gcloud", "container", "images", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud container images list"),
        ),
        (
            &[
                "gcloud",
                "container",
                "images",
                "describe",
                "gcr.io/my-project/my-image",
            ],
            &[
                "gcloud",
                "container",
                "images",
                "describe",
                "gcr.io/my-project/my-image",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud container images describe"),
        ),
        (
            &[
                "gcloud",
                "container",
                "images",
                "list-tags",
                "gcr.io/my-project/my-image",
            ],
            &[
                "gcloud",
                "container",
                "images",
                "list-tags",
                "gcr.io/my-project/my-image",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud container images list-tags"),
        ),
        (
            &[
                "gcloud",
                "container",
                "images",
                "delete",
                "gcr.io/my-project/my-image",
            ],
            &[
                "gcloud",
                "container",
                "images",
                "delete",
                "gcr.io/my-project/my-image",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud container images delete"),
        ),
        (
            &[
                "gcloud",
                "container",
                "node-pools",
                "list",
                "--cluster=my-cluster",
            ],
            &[
                "gcloud",
                "container",
                "node-pools",
                "list",
                "--cluster=my-cluster",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud container node-pools list"),
        ),
        (
            &[
                "gcloud",
                "container",
                "node-pools",
                "describe",
                "my-pool",
                "--cluster=my-cluster",
            ],
            &[
                "gcloud",
                "container",
                "node-pools",
                "describe",
                "my-pool",
                "--cluster=my-cluster",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud container node-pools describe"),
        ),
        (
            &[
                "gcloud",
                "container",
                "node-pools",
                "create",
                "my-pool",
                "--cluster=my-cluster",
            ],
            &[
                "gcloud",
                "container",
                "node-pools",
                "create",
                "my-pool",
                "--cluster=my-cluster",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud container node-pools create"),
        ),
        (
            &[
                "gcloud",
                "container",
                "node-pools",
                "delete",
                "my-pool",
                "--cluster=my-cluster",
            ],
            &[
                "gcloud",
                "container",
                "node-pools",
                "delete",
                "my-pool",
                "--cluster=my-cluster",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud container node-pools delete"),
        ),
        (
            &["gcloud", "iam", "roles", "list"],
            &["gcloud", "iam", "roles", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud iam roles list"),
        ),
        (
            &["gcloud", "iam", "roles", "describe", "roles/editor"],
            &["gcloud", "iam", "roles", "describe", "roles/editor"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud iam roles describe"),
        ),
        (
            &[
                "gcloud",
                "iam",
                "roles",
                "create",
                "my-role",
                "--project=my-project",
                "--file=role.yaml",
            ],
            &[
                "gcloud",
                "iam",
                "roles",
                "create",
                "my-role",
                "--project=my-project",
                "--file=role.yaml",
            ],
            &[false, false, false, false, false, false, false],
            "ask",
            Some("gcloud iam roles create"),
        ),
        (
            &[
                "gcloud",
                "iam",
                "roles",
                "delete",
                "my-role",
                "--project=my-project",
            ],
            &[
                "gcloud",
                "iam",
                "roles",
                "delete",
                "my-role",
                "--project=my-project",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud iam roles delete"),
        ),
        (
            &[
                "gcloud",
                "iam",
                "list-grantable-roles",
                "//cloudresourcemanager.googleapis.com/projects/my-project",
            ],
            &[
                "gcloud",
                "iam",
                "list-grantable-roles",
                "//cloudresourcemanager.googleapis.com/projects/my-project",
            ],
            &[false, false, false, false],
            "allow",
            Some("gcloud iam list-grantable-roles"),
        ),
        (
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "describe",
                "sa@project.iam.gserviceaccount.com",
            ],
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "describe",
                "sa@project.iam.gserviceaccount.com",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud iam service-accounts describe"),
        ),
        (
            &["gcloud", "iam", "service-accounts", "create", "my-sa"],
            &["gcloud", "iam", "service-accounts", "create", "my-sa"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud iam service-accounts create"),
        ),
        (
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "delete",
                "sa@project.iam.gserviceaccount.com",
            ],
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "delete",
                "sa@project.iam.gserviceaccount.com",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud iam service-accounts delete"),
        ),
        (
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "add-iam-policy-binding",
                "sa@project.iam.gserviceaccount.com",
                "--member=user:foo",
                "--role=roles/iam.serviceAccountUser",
            ],
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "add-iam-policy-binding",
                "sa@project.iam.gserviceaccount.com",
                "--member=user:foo",
                "--role=roles/iam.serviceAccountUser",
            ],
            &[false, false, false, false, false, false, false],
            "ask",
            Some("gcloud iam service-accounts add-iam-policy-binding"),
        ),
        (
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "set-iam-policy",
                "sa@project.iam.gserviceaccount.com",
                "policy.json",
            ],
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "set-iam-policy",
                "sa@project.iam.gserviceaccount.com",
                "policy.json",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud iam service-accounts set-iam-policy"),
        ),
        (
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "keys",
                "list",
                "--iam-account=sa@project.iam.gserviceaccount.com",
            ],
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "keys",
                "list",
                "--iam-account=sa@project.iam.gserviceaccount.com",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud iam service-accounts keys"),
        ),
        (
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "keys",
                "create",
                "key.json",
                "--iam-account=sa@project.iam.gserviceaccount.com",
            ],
            &[
                "gcloud",
                "iam",
                "service-accounts",
                "keys",
                "create",
                "key.json",
                "--iam-account=sa@project.iam.gserviceaccount.com",
            ],
            &[false, false, false, false, false, false, false],
            "ask",
            Some("gcloud iam service-accounts keys"),
        ),
        (
            &["gcloud", "app", "deploy"],
            &["gcloud", "app", "deploy"],
            &[false, false, false],
            "ask",
            Some("gcloud app deploy"),
        ),
        (
            &["gcloud", "app", "deploy", "app.yaml"],
            &["gcloud", "app", "deploy", "app.yaml"],
            &[false, false, false, false],
            "ask",
            Some("gcloud app deploy app.yaml"),
        ),
        (
            &["gcloud", "app", "versions", "list"],
            &["gcloud", "app", "versions", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud app versions list"),
        ),
        (
            &[
                "gcloud",
                "app",
                "versions",
                "describe",
                "v1",
                "--service=default",
            ],
            &[
                "gcloud",
                "app",
                "versions",
                "describe",
                "v1",
                "--service=default",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud app versions describe"),
        ),
        (
            &[
                "gcloud",
                "app",
                "versions",
                "delete",
                "v1",
                "--service=default",
            ],
            &[
                "gcloud",
                "app",
                "versions",
                "delete",
                "v1",
                "--service=default",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud app versions delete"),
        ),
        (
            &["gcloud", "app", "browse"],
            &["gcloud", "app", "browse"],
            &[false, false, false],
            "ask",
            Some("gcloud app browse"),
        ),
        (
            &["gcloud", "app", "create"],
            &["gcloud", "app", "create"],
            &[false, false, false],
            "ask",
            Some("gcloud app create"),
        ),
        (
            &["gcloud", "app", "logs", "read"],
            &["gcloud", "app", "logs", "read"],
            &[false, false, false, false],
            "allow",
            Some("gcloud app logs read"),
        ),
        (
            &["gcloud", "app", "describe"],
            &["gcloud", "app", "describe"],
            &[false, false, false],
            "allow",
            Some("gcloud app describe"),
        ),
        (
            &["gcloud", "app", "services", "list"],
            &["gcloud", "app", "services", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud app services list"),
        ),
        (
            &["gcloud", "app", "services", "describe", "default"],
            &["gcloud", "app", "services", "describe", "default"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud app services describe"),
        ),
        (
            &["gcloud", "projects", "create", "my-new-project"],
            &["gcloud", "projects", "create", "my-new-project"],
            &[false, false, false, false],
            "ask",
            Some("gcloud projects create my-new-project"),
        ),
        (
            &["gcloud", "projects", "delete", "my-project"],
            &["gcloud", "projects", "delete", "my-project"],
            &[false, false, false, false],
            "ask",
            Some("gcloud projects delete my-project"),
        ),
        (
            &["gcloud", "projects", "undelete", "my-project"],
            &["gcloud", "projects", "undelete", "my-project"],
            &[false, false, false, false],
            "ask",
            Some("gcloud projects undelete my-project"),
        ),
        (
            &["gcloud", "secrets", "versions", "list", "my-secret"],
            &["gcloud", "secrets", "versions", "list", "my-secret"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud secrets versions list"),
        ),
        (
            &[
                "gcloud",
                "secrets",
                "versions",
                "describe",
                "1",
                "--secret=my-secret",
            ],
            &[
                "gcloud",
                "secrets",
                "versions",
                "describe",
                "1",
                "--secret=my-secret",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud secrets versions describe"),
        ),
        (
            &[
                "gcloud",
                "secrets",
                "versions",
                "access",
                "1",
                "--secret=my-secret",
            ],
            &[
                "gcloud",
                "secrets",
                "versions",
                "access",
                "1",
                "--secret=my-secret",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud secrets versions access"),
        ),
        (
            &[
                "gcloud",
                "secrets",
                "versions",
                "destroy",
                "1",
                "--secret=my-secret",
            ],
            &[
                "gcloud",
                "secrets",
                "versions",
                "destroy",
                "1",
                "--secret=my-secret",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud secrets versions destroy"),
        ),
        (
            &[
                "gcloud",
                "secrets",
                "add-iam-policy-binding",
                "my-secret",
                "--member=user:foo",
                "--role=roles/secretmanager.secretAccessor",
            ],
            &[
                "gcloud",
                "secrets",
                "add-iam-policy-binding",
                "my-secret",
                "--member=user:foo",
                "--role=roles/secretmanager.secretAccessor",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud secrets add-iam-policy-binding my-secret"),
        ),
        (
            &["gcloud", "functions", "describe", "my-function"],
            &["gcloud", "functions", "describe", "my-function"],
            &[false, false, false, false],
            "allow",
            Some("gcloud functions describe my-function"),
        ),
        (
            &["gcloud", "functions", "logs", "read", "my-function"],
            &["gcloud", "functions", "logs", "read", "my-function"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud functions logs read"),
        ),
        (
            &["gcloud", "functions", "deploy", "my-function"],
            &["gcloud", "functions", "deploy", "my-function"],
            &[false, false, false, false],
            "ask",
            Some("gcloud functions deploy my-function"),
        ),
        (
            &["gcloud", "functions", "delete", "my-function"],
            &["gcloud", "functions", "delete", "my-function"],
            &[false, false, false, false],
            "ask",
            Some("gcloud functions delete my-function"),
        ),
        (
            &["gcloud", "functions", "call", "my-function"],
            &["gcloud", "functions", "call", "my-function"],
            &[false, false, false, false],
            "ask",
            Some("gcloud functions call my-function"),
        ),
        (
            &["gcloud", "logging", "read", "severity>=ERROR"],
            &["gcloud", "logging", "read", "'severity>=ERROR'"],
            &[false, false, false, false],
            "allow",
            Some("gcloud logging read severity>=ERROR"),
        ),
        (
            &["gcloud", "logging", "logs", "list"],
            &["gcloud", "logging", "logs", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud logging logs list"),
        ),
        (
            &[
                "gcloud",
                "logging",
                "logs",
                "list",
                "--bucket=my-bucket",
                "--location=us-central1",
            ],
            &[
                "gcloud",
                "logging",
                "logs",
                "list",
                "--bucket=my-bucket",
                "--location=us-central1",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud logging logs list"),
        ),
        (
            &[
                "gcloud",
                "logging",
                "logs",
                "list",
                "--filter='logName:syslog'",
            ],
            &[
                "gcloud",
                "logging",
                "logs",
                "list",
                "--filter='logName:syslog'",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud logging logs list"),
        ),
        (
            &["gcloud", "logging", "logs", "list", "--limit=100"],
            &["gcloud", "logging", "logs", "list", "--limit=100"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud logging logs list"),
        ),
        (
            &["gcloud", "logging", "logs", "list", "--sort-by='timestamp'"],
            &["gcloud", "logging", "logs", "list", "--sort-by='timestamp'"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud logging logs list"),
        ),
        (
            &["gcloud", "logging", "logs", "list", "--verbosity=debug"],
            &["gcloud", "logging", "logs", "list", "--verbosity=debug"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud logging logs list"),
        ),
        (
            &["gcloud", "logging", "logs", "delete", "my-log"],
            &["gcloud", "logging", "logs", "delete", "my-log"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud logging logs delete"),
        ),
        (
            &["gcloud", "logging", "write", "my-log", "message"],
            &["gcloud", "logging", "write", "my-log", "'message'"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud logging write my-log"),
        ),
        (
            &["gcloud", "dns", "managed-zones", "list"],
            &["gcloud", "dns", "managed-zones", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud dns managed-zones list"),
        ),
        (
            &["gcloud", "dns", "managed-zones", "describe", "my-zone"],
            &["gcloud", "dns", "managed-zones", "describe", "my-zone"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud dns managed-zones describe"),
        ),
        (
            &[
                "gcloud",
                "dns",
                "managed-zones",
                "create",
                "my-zone",
                "--dns-name=example.com",
            ],
            &[
                "gcloud",
                "dns",
                "managed-zones",
                "create",
                "my-zone",
                "--dns-name=example.com",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud dns managed-zones create"),
        ),
        (
            &["gcloud", "dns", "managed-zones", "delete", "my-zone"],
            &["gcloud", "dns", "managed-zones", "delete", "my-zone"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud dns managed-zones delete"),
        ),
        (
            &["gcloud", "dns", "record-sets", "list", "--zone=my-zone"],
            &["gcloud", "dns", "record-sets", "list", "--zone=my-zone"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud dns record-sets list"),
        ),
        (
            &[
                "gcloud",
                "dns",
                "record-sets",
                "describe",
                "www",
                "--zone=my-zone",
                "--type=A",
            ],
            &[
                "gcloud",
                "dns",
                "record-sets",
                "describe",
                "www",
                "--zone=my-zone",
                "--type=A",
            ],
            &[false, false, false, false, false, false, false],
            "allow",
            Some("gcloud dns record-sets describe"),
        ),
        (
            &[
                "gcloud",
                "dns",
                "record-sets",
                "create",
                "www",
                "--zone=my-zone",
                "--type=A",
                "--rrdatas=1.2.3.4",
            ],
            &[
                "gcloud",
                "dns",
                "record-sets",
                "create",
                "www",
                "--zone=my-zone",
                "--type=A",
                "--rrdatas=1.2.3.4",
            ],
            &[false, false, false, false, false, false, false, false],
            "ask",
            Some("gcloud dns record-sets create"),
        ),
        (
            &[
                "gcloud",
                "dns",
                "record-sets",
                "delete",
                "www",
                "--zone=my-zone",
                "--type=A",
            ],
            &[
                "gcloud",
                "dns",
                "record-sets",
                "delete",
                "www",
                "--zone=my-zone",
                "--type=A",
            ],
            &[false, false, false, false, false, false, false],
            "ask",
            Some("gcloud dns record-sets delete"),
        ),
        (
            &["gcloud", "storage", "buckets", "list"],
            &["gcloud", "storage", "buckets", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud storage buckets list"),
        ),
        (
            &["gcloud", "storage", "buckets", "describe", "gs://my-bucket"],
            &["gcloud", "storage", "buckets", "describe", "gs://my-bucket"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud storage buckets describe"),
        ),
        (
            &["gcloud", "storage", "buckets", "create", "gs://my-bucket"],
            &["gcloud", "storage", "buckets", "create", "gs://my-bucket"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud storage buckets create"),
        ),
        (
            &["gcloud", "storage", "buckets", "delete", "gs://my-bucket"],
            &["gcloud", "storage", "buckets", "delete", "gs://my-bucket"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud storage buckets delete"),
        ),
        (
            &["gcloud", "storage", "objects", "list", "gs://my-bucket"],
            &["gcloud", "storage", "objects", "list", "gs://my-bucket"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud storage objects list"),
        ),
        (
            &[
                "gcloud",
                "storage",
                "objects",
                "describe",
                "gs://my-bucket/my-object",
            ],
            &[
                "gcloud",
                "storage",
                "objects",
                "describe",
                "gs://my-bucket/my-object",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud storage objects describe"),
        ),
        (
            &["gcloud", "storage", "cp", "gs://src/file", "gs://dst/file"],
            &["gcloud", "storage", "cp", "gs://src/file", "gs://dst/file"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud storage cp"),
        ),
        (
            &["gcloud", "storage", "rm", "gs://my-bucket/my-object"],
            &["gcloud", "storage", "rm", "gs://my-bucket/my-object"],
            &[false, false, false, false],
            "ask",
            Some("gcloud storage rm"),
        ),
        (
            &[
                "gcloud",
                "run",
                "services",
                "describe",
                "my-service",
                "--region=us-central1",
            ],
            &[
                "gcloud",
                "run",
                "services",
                "describe",
                "my-service",
                "--region=us-central1",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud run services describe"),
        ),
        (
            &[
                "gcloud",
                "run",
                "services",
                "update",
                "my-service",
                "--region=us-central1",
                "--memory=512Mi",
            ],
            &[
                "gcloud",
                "run",
                "services",
                "update",
                "my-service",
                "--region=us-central1",
                "--memory=512Mi",
            ],
            &[false, false, false, false, false, false, false],
            "ask",
            Some("gcloud run services update"),
        ),
        (
            &[
                "gcloud",
                "run",
                "services",
                "delete",
                "my-service",
                "--region=us-central1",
            ],
            &[
                "gcloud",
                "run",
                "services",
                "delete",
                "my-service",
                "--region=us-central1",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud run services delete"),
        ),
        (
            &[
                "gcloud",
                "run",
                "deploy",
                "my-service",
                "--image=gcr.io/my-project/my-image",
            ],
            &[
                "gcloud",
                "run",
                "deploy",
                "my-service",
                "--image=gcr.io/my-project/my-image",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud run deploy my-service"),
        ),
        (
            &["gcloud", "run", "revisions", "list", "--service=my-service"],
            &["gcloud", "run", "revisions", "list", "--service=my-service"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud run revisions list"),
        ),
        (
            &["gcloud", "run", "revisions", "describe", "my-revision"],
            &["gcloud", "run", "revisions", "describe", "my-revision"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud run revisions describe"),
        ),
        (
            &["gcloud", "artifacts", "repositories", "list"],
            &["gcloud", "artifacts", "repositories", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud artifacts repositories list"),
        ),
        (
            &[
                "gcloud",
                "artifacts",
                "repositories",
                "describe",
                "my-repo",
                "--location=us-central1",
            ],
            &[
                "gcloud",
                "artifacts",
                "repositories",
                "describe",
                "my-repo",
                "--location=us-central1",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud artifacts repositories describe"),
        ),
        (
            &[
                "gcloud",
                "artifacts",
                "repositories",
                "create",
                "my-repo",
                "--location=us-central1",
            ],
            &[
                "gcloud",
                "artifacts",
                "repositories",
                "create",
                "my-repo",
                "--location=us-central1",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud artifacts repositories create"),
        ),
        (
            &[
                "gcloud",
                "artifacts",
                "repositories",
                "delete",
                "my-repo",
                "--location=us-central1",
            ],
            &[
                "gcloud",
                "artifacts",
                "repositories",
                "delete",
                "my-repo",
                "--location=us-central1",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud artifacts repositories delete"),
        ),
        (
            &[
                "gcloud",
                "artifacts",
                "docker",
                "images",
                "list",
                "us-central1-docker.pkg.dev/my-project/my-repo",
            ],
            &[
                "gcloud",
                "artifacts",
                "docker",
                "images",
                "list",
                "us-central1-docker.pkg.dev/my-project/my-repo",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud artifacts docker images"),
        ),
        (
            &[
                "gcloud",
                "artifacts",
                "docker",
                "tags",
                "list",
                "us-central1-docker.pkg.dev/my-project/my-repo/my-image",
            ],
            &[
                "gcloud",
                "artifacts",
                "docker",
                "tags",
                "list",
                "us-central1-docker.pkg.dev/my-project/my-repo/my-image",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud artifacts docker tags"),
        ),
        (
            &[
                "gcloud",
                "artifacts",
                "docker",
                "tags",
                "delete",
                "us-central1-docker.pkg.dev/my-project/my-repo/my-image:v1",
            ],
            &[
                "gcloud",
                "artifacts",
                "docker",
                "tags",
                "delete",
                "us-central1-docker.pkg.dev/my-project/my-repo/my-image:v1",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud artifacts docker tags"),
        ),
        (
            &["gcloud", "beta", "run", "services", "list"],
            &["gcloud", "beta", "run", "services", "list"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud beta run services"),
        ),
        (
            &[
                "gcloud",
                "beta",
                "run",
                "services",
                "describe",
                "my-service",
            ],
            &[
                "gcloud",
                "beta",
                "run",
                "services",
                "describe",
                "my-service",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud beta run services"),
        ),
        (
            &["gcloud", "beta", "run", "services", "update", "my-service"],
            &["gcloud", "beta", "run", "services", "update", "my-service"],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud beta run services"),
        ),
        (
            &["gcloud", "beta", "run", "services", "delete", "my-service"],
            &["gcloud", "beta", "run", "services", "delete", "my-service"],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud beta run services"),
        ),
        (
            &["gcloud", "beta", "compute", "instances", "list"],
            &["gcloud", "beta", "compute", "instances", "list"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud beta compute instances"),
        ),
        (
            &[
                "gcloud",
                "beta",
                "compute",
                "instances",
                "describe",
                "my-instance",
            ],
            &[
                "gcloud",
                "beta",
                "compute",
                "instances",
                "describe",
                "my-instance",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud beta compute instances"),
        ),
        (
            &["gcloud", "certificate-manager", "certificates", "list"],
            &["gcloud", "certificate-manager", "certificates", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud certificate-manager certificates list"),
        ),
        (
            &[
                "gcloud",
                "certificate-manager",
                "certificates",
                "describe",
                "my-cert",
            ],
            &[
                "gcloud",
                "certificate-manager",
                "certificates",
                "describe",
                "my-cert",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud certificate-manager certificates describe"),
        ),
        (
            &[
                "gcloud",
                "certificate-manager",
                "certificates",
                "create",
                "my-cert",
            ],
            &[
                "gcloud",
                "certificate-manager",
                "certificates",
                "create",
                "my-cert",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud certificate-manager certificates create"),
        ),
        (
            &["gcloud", "certificate-manager", "trust-configs", "list"],
            &["gcloud", "certificate-manager", "trust-configs", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud certificate-manager trust-configs list"),
        ),
        (
            &[
                "gcloud",
                "certificate-manager",
                "trust-configs",
                "describe",
                "my-config",
            ],
            &[
                "gcloud",
                "certificate-manager",
                "trust-configs",
                "describe",
                "my-config",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud certificate-manager trust-configs describe"),
        ),
        (
            &["gcloud", "network-security", "server-tls-policies", "list"],
            &["gcloud", "network-security", "server-tls-policies", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud network-security server-tls-policies list"),
        ),
        (
            &[
                "gcloud",
                "network-security",
                "server-tls-policies",
                "describe",
                "my-policy",
            ],
            &[
                "gcloud",
                "network-security",
                "server-tls-policies",
                "describe",
                "my-policy",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud network-security server-tls-policies describe"),
        ),
        (
            &[
                "gcloud",
                "network-security",
                "server-tls-policies",
                "create",
                "my-policy",
            ],
            &[
                "gcloud",
                "network-security",
                "server-tls-policies",
                "create",
                "my-policy",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud network-security server-tls-policies create"),
        ),
        (
            &[
                "gcloud",
                "network-security",
                "gateway-security-policies",
                "list",
            ],
            &[
                "gcloud",
                "network-security",
                "gateway-security-policies",
                "list",
            ],
            &[false, false, false, false],
            "allow",
            Some("gcloud network-security gateway-security-policies list"),
        ),
        (
            &[
                "gcloud",
                "network-security",
                "gateway-security-policies",
                "describe",
                "my-policy",
            ],
            &[
                "gcloud",
                "network-security",
                "gateway-security-policies",
                "describe",
                "my-policy",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud network-security gateway-security-policies describe"),
        ),
        (
            &["gcloud", "iap", "settings", "get", "--project=my-project"],
            &["gcloud", "iap", "settings", "get", "--project=my-project"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud iap settings get"),
        ),
        (
            &[
                "gcloud",
                "iap",
                "settings",
                "set",
                "iap-settings.yaml",
                "--project=my-project",
            ],
            &[
                "gcloud",
                "iap",
                "settings",
                "set",
                "iap-settings.yaml",
                "--project=my-project",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud iap settings set"),
        ),
        (
            &[
                "gcloud",
                "iap",
                "web",
                "get-iam-policy",
                "--resource-type=backend-services",
                "--service=my-service",
            ],
            &[
                "gcloud",
                "iap",
                "web",
                "get-iam-policy",
                "--resource-type=backend-services",
                "--service=my-service",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud iap web get-iam-policy"),
        ),
        (
            &[
                "gcloud",
                "iap",
                "web",
                "set-iam-policy",
                "policy.json",
                "--resource-type=backend-services",
            ],
            &[
                "gcloud",
                "iap",
                "web",
                "set-iam-policy",
                "policy.json",
                "--resource-type=backend-services",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud iap web set-iam-policy"),
        ),
        (
            &["gcloud", "iap", "tcp", "tunnels", "list"],
            &["gcloud", "iap", "tcp", "tunnels", "list"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud iap tcp tunnels"),
        ),
        (
            &["gcloud", "sql", "instances", "list"],
            &["gcloud", "sql", "instances", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud sql instances list"),
        ),
        (
            &["gcloud", "sql", "instances", "describe", "my-instance"],
            &["gcloud", "sql", "instances", "describe", "my-instance"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud sql instances describe"),
        ),
        (
            &["gcloud", "sql", "instances", "create", "my-instance"],
            &["gcloud", "sql", "instances", "create", "my-instance"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud sql instances create"),
        ),
        (
            &["gcloud", "sql", "instances", "delete", "my-instance"],
            &["gcloud", "sql", "instances", "delete", "my-instance"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud sql instances delete"),
        ),
        (
            &[
                "gcloud",
                "sql",
                "databases",
                "list",
                "--instance=my-instance",
            ],
            &[
                "gcloud",
                "sql",
                "databases",
                "list",
                "--instance=my-instance",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud sql databases list"),
        ),
        (
            &[
                "gcloud",
                "sql",
                "databases",
                "describe",
                "my-db",
                "--instance=my-instance",
            ],
            &[
                "gcloud",
                "sql",
                "databases",
                "describe",
                "my-db",
                "--instance=my-instance",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud sql databases describe"),
        ),
        (
            &[
                "gcloud",
                "sql",
                "databases",
                "create",
                "my-db",
                "--instance=my-instance",
            ],
            &[
                "gcloud",
                "sql",
                "databases",
                "create",
                "my-db",
                "--instance=my-instance",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud sql databases create"),
        ),
        (
            &["gcloud", "sql", "backups", "list", "--instance=my-instance"],
            &["gcloud", "sql", "backups", "list", "--instance=my-instance"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud sql backups list"),
        ),
        (
            &[
                "gcloud",
                "sql",
                "backups",
                "describe",
                "12345",
                "--instance=my-instance",
            ],
            &[
                "gcloud",
                "sql",
                "backups",
                "describe",
                "12345",
                "--instance=my-instance",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud sql backups describe"),
        ),
        (
            &[
                "gcloud",
                "sql",
                "backups",
                "create",
                "--instance=my-instance",
            ],
            &[
                "gcloud",
                "sql",
                "backups",
                "create",
                "--instance=my-instance",
            ],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud sql backups create"),
        ),
        (
            &[
                "gcloud",
                "sql",
                "export",
                "sql",
                "my-instance",
                "gs://my-bucket/dump.sql",
            ],
            &[
                "gcloud",
                "sql",
                "export",
                "sql",
                "my-instance",
                "gs://my-bucket/dump.sql",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud sql export sql"),
        ),
        (
            &[
                "gcloud",
                "sql",
                "export",
                "sql",
                "my-instance",
                "gs://my-bucket/dump.sql",
                "--async",
            ],
            &[
                "gcloud",
                "sql",
                "export",
                "sql",
                "my-instance",
                "gs://my-bucket/dump.sql",
                "--async",
            ],
            &[false, false, false, false, false, false, false],
            "ask",
            Some("gcloud sql export sql"),
        ),
        (
            &[
                "gcloud",
                "sql",
                "export",
                "sql",
                "my-instance",
                "gs://my-bucket/dump.sql",
                "--database=mydb",
            ],
            &[
                "gcloud",
                "sql",
                "export",
                "sql",
                "my-instance",
                "gs://my-bucket/dump.sql",
                "--database=mydb",
            ],
            &[false, false, false, false, false, false, false],
            "ask",
            Some("gcloud sql export sql"),
        ),
        (
            &[
                "gcloud",
                "sql",
                "import",
                "sql",
                "my-instance",
                "gs://my-bucket/dump.sql",
            ],
            &[
                "gcloud",
                "sql",
                "import",
                "sql",
                "my-instance",
                "gs://my-bucket/dump.sql",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud sql import sql"),
        ),
        (
            &["gcloud", "kms", "keyrings", "list", "--location=global"],
            &["gcloud", "kms", "keyrings", "list", "--location=global"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud kms keyrings list"),
        ),
        (
            &[
                "gcloud",
                "kms",
                "keyrings",
                "describe",
                "my-keyring",
                "--location=global",
            ],
            &[
                "gcloud",
                "kms",
                "keyrings",
                "describe",
                "my-keyring",
                "--location=global",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud kms keyrings describe"),
        ),
        (
            &[
                "gcloud",
                "kms",
                "keyrings",
                "create",
                "my-keyring",
                "--location=global",
            ],
            &[
                "gcloud",
                "kms",
                "keyrings",
                "create",
                "my-keyring",
                "--location=global",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud kms keyrings create"),
        ),
        (
            &[
                "gcloud",
                "kms",
                "keys",
                "list",
                "--keyring=my-keyring",
                "--location=global",
            ],
            &[
                "gcloud",
                "kms",
                "keys",
                "list",
                "--keyring=my-keyring",
                "--location=global",
            ],
            &[false, false, false, false, false, false],
            "allow",
            Some("gcloud kms keys list"),
        ),
        (
            &[
                "gcloud",
                "kms",
                "keys",
                "describe",
                "my-key",
                "--keyring=my-keyring",
                "--location=global",
            ],
            &[
                "gcloud",
                "kms",
                "keys",
                "describe",
                "my-key",
                "--keyring=my-keyring",
                "--location=global",
            ],
            &[false, false, false, false, false, false, false],
            "allow",
            Some("gcloud kms keys describe"),
        ),
        (
            &[
                "gcloud",
                "kms",
                "keys",
                "create",
                "my-key",
                "--keyring=my-keyring",
                "--location=global",
                "--purpose=encryption",
            ],
            &[
                "gcloud",
                "kms",
                "keys",
                "create",
                "my-key",
                "--keyring=my-keyring",
                "--location=global",
                "--purpose=encryption",
            ],
            &[false, false, false, false, false, false, false, false],
            "ask",
            Some("gcloud kms keys create"),
        ),
        (
            &[
                "gcloud",
                "kms",
                "decrypt",
                "--key=my-key",
                "--keyring=my-keyring",
                "--location=global",
                "--ciphertext-file=cipher.enc",
                "--plaintext-file=plain.txt",
            ],
            &[
                "gcloud",
                "kms",
                "decrypt",
                "--key=my-key",
                "--keyring=my-keyring",
                "--location=global",
                "--ciphertext-file=cipher.enc",
                "--plaintext-file=plain.txt",
            ],
            &[false, false, false, false, false, false, false, false],
            "ask",
            Some("gcloud kms decrypt"),
        ),
        (
            &[
                "gcloud",
                "kms",
                "encrypt",
                "--key=my-key",
                "--keyring=my-keyring",
                "--location=global",
                "--plaintext-file=plain.txt",
                "--ciphertext-file=cipher.enc",
            ],
            &[
                "gcloud",
                "kms",
                "encrypt",
                "--key=my-key",
                "--keyring=my-keyring",
                "--location=global",
                "--plaintext-file=plain.txt",
                "--ciphertext-file=cipher.enc",
            ],
            &[false, false, false, false, false, false, false, false],
            "ask",
            Some("gcloud kms encrypt"),
        ),
        (
            &["gcloud", "pubsub", "topics", "list"],
            &["gcloud", "pubsub", "topics", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud pubsub topics list"),
        ),
        (
            &["gcloud", "pubsub", "topics", "describe", "my-topic"],
            &["gcloud", "pubsub", "topics", "describe", "my-topic"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud pubsub topics describe"),
        ),
        (
            &["gcloud", "pubsub", "topics", "create", "my-topic"],
            &["gcloud", "pubsub", "topics", "create", "my-topic"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud pubsub topics create"),
        ),
        (
            &["gcloud", "pubsub", "topics", "delete", "my-topic"],
            &["gcloud", "pubsub", "topics", "delete", "my-topic"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud pubsub topics delete"),
        ),
        (
            &[
                "gcloud",
                "pubsub",
                "topics",
                "publish",
                "my-topic",
                "--message='hello'",
            ],
            &[
                "gcloud",
                "pubsub",
                "topics",
                "publish",
                "my-topic",
                "--message='hello'",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud pubsub topics publish"),
        ),
        (
            &["gcloud", "pubsub", "subscriptions", "list"],
            &["gcloud", "pubsub", "subscriptions", "list"],
            &[false, false, false, false],
            "allow",
            Some("gcloud pubsub subscriptions list"),
        ),
        (
            &["gcloud", "pubsub", "subscriptions", "describe", "my-sub"],
            &["gcloud", "pubsub", "subscriptions", "describe", "my-sub"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud pubsub subscriptions describe"),
        ),
        (
            &[
                "gcloud",
                "pubsub",
                "subscriptions",
                "create",
                "my-sub",
                "--topic=my-topic",
            ],
            &[
                "gcloud",
                "pubsub",
                "subscriptions",
                "create",
                "my-sub",
                "--topic=my-topic",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud pubsub subscriptions create"),
        ),
        (
            &["gcloud", "pubsub", "subscriptions", "pull", "my-sub"],
            &["gcloud", "pubsub", "subscriptions", "pull", "my-sub"],
            &[false, false, false, false, false],
            "ask",
            Some("gcloud pubsub subscriptions pull"),
        ),
        (
            &[
                "gcloud",
                "--project=my-project",
                "compute",
                "instances",
                "list",
            ],
            &[
                "gcloud",
                "--project=my-project",
                "compute",
                "instances",
                "list",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute instances list"),
        ),
        (
            &["gcloud", "--format=json", "compute", "instances", "list"],
            &["gcloud", "--format=json", "compute", "instances", "list"],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute instances list"),
        ),
        (
            &[
                "gcloud",
                "--account=user@example.com",
                "compute",
                "instances",
                "list",
            ],
            &[
                "gcloud",
                "--account=user@example.com",
                "compute",
                "instances",
                "list",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute instances list"),
        ),
        (
            &[
                "gcloud",
                "--configuration=my-config",
                "compute",
                "instances",
                "list",
            ],
            &[
                "gcloud",
                "--configuration=my-config",
                "compute",
                "instances",
                "list",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute instances list"),
        ),
        (
            &[
                "gcloud",
                "--region=us-central1",
                "compute",
                "instances",
                "list",
            ],
            &[
                "gcloud",
                "--region=us-central1",
                "compute",
                "instances",
                "list",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute instances list"),
        ),
        (
            &[
                "gcloud",
                "--zone=us-central1-a",
                "compute",
                "instances",
                "list",
            ],
            &[
                "gcloud",
                "--zone=us-central1-a",
                "compute",
                "instances",
                "list",
            ],
            &[false, false, false, false, false],
            "allow",
            Some("gcloud compute instances list"),
        ),
        (
            &[
                "gcloud",
                "--project=my-project",
                "--format=json",
                "compute",
                "instances",
                "describe",
                "my-instance",
            ],
            &[
                "gcloud",
                "--project=my-project",
                "--format=json",
                "compute",
                "instances",
                "describe",
                "my-instance",
            ],
            &[false, false, false, false, false, false, false],
            "allow",
            Some("gcloud compute instances describe"),
        ),
        (
            &[
                "gcloud",
                "--project=my-project",
                "compute",
                "instances",
                "delete",
                "my-instance",
            ],
            &[
                "gcloud",
                "--project=my-project",
                "compute",
                "instances",
                "delete",
                "my-instance",
            ],
            &[false, false, false, false, false, false],
            "ask",
            Some("gcloud compute instances delete"),
        ),
        (
            &["gcloud", "help"],
            &["gcloud", "help"],
            &[false, false],
            "allow",
            Some("gcloud help"),
        ),
        (
            &["gcloud", "help", "compute"],
            &["gcloud", "help", "compute"],
            &[false, false, false],
            "allow",
            Some("gcloud help compute"),
        ),
        (
            &["gcloud", "help", "compute", "instances"],
            &["gcloud", "help", "compute", "instances"],
            &[false, false, false, false],
            "allow",
            Some("gcloud help compute instances"),
        ),
        (
            &["gcloud", "info"],
            &["gcloud", "info"],
            &[false, false],
            "allow",
            Some("gcloud info"),
        ),
        (
            &["gcloud", "info", "--run-diagnostics"],
            &["gcloud", "info", "--run-diagnostics"],
            &[false, false, false],
            "allow",
            Some("gcloud info"),
        ),
        (
            &["gcloud", "info", "--show-log"],
            &["gcloud", "info", "--show-log"],
            &[false, false, false],
            "allow",
            Some("gcloud info"),
        ),
        (
            &["gcloud", "version"],
            &["gcloud", "version"],
            &[false, false],
            "allow",
            Some("gcloud version"),
        ),
        (
            &["gcloud", "version", "--help"],
            &["gcloud", "version", "--help"],
            &[false, false, false],
            "allow",
            Some("gcloud version"),
        ),
        (
            &["gcloud", "init"],
            &["gcloud", "init"],
            &[false, false],
            "ask",
            Some("gcloud init"),
        ),
        (
            &["gcloud", "init", "--skip-diagnostics"],
            &["gcloud", "init", "--skip-diagnostics"],
            &[false, false, false],
            "ask",
            Some("gcloud init"),
        ),
        (
            &["gcloud", "feedback"],
            &["gcloud", "feedback"],
            &[false, false],
            "ask",
            Some("gcloud feedback"),
        ),
        (
            &["gcloud", "topic", "configurations"],
            &["gcloud", "topic", "configurations"],
            &[false, false, false],
            "allow",
            Some("gcloud topic configurations"),
        ),
        (
            &["gcloud", "compute", "instances"],
            &["gcloud", "compute", "instances"],
            &[false, false, false],
            "ask",
            Some("gcloud compute instances"),
        ),
        (
            &["gcloud", "compute"],
            &["gcloud", "compute"],
            &[false, false],
            "ask",
            Some("gcloud compute"),
        ),
        (&["gcloud"], &["gcloud"], &[false], "ask", Some("gcloud")),
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
