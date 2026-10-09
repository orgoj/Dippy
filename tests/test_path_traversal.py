"""Path rules must match the path a `..` component actually reaches."""

import pytest

from conftest import is_approved, needs_confirmation
from dippy.core.config import (
    Config,
    Rule,
    _match_redirect,
    match_edit,
    match_read,
    parse_config,
)

MATCHERS = [
    ("edit", match_edit),
    ("read", match_read),
    ("redirect", _match_redirect),
]


def _config(kind, rules):
    return Config(**{f"{kind}_rules": rules})


@pytest.mark.parametrize("kind,matcher", MATCHERS)
def test_absolute_dotdot_cannot_escape_allowed_tree(kind, matcher, tmp_path):
    cfg = _config(kind, [Rule("allow", "/data/box/**")])
    assert matcher("/data/box/sub/x", cfg, tmp_path) is not None
    assert matcher("/data/box/sub/../../secret", cfg, tmp_path) is None
    assert matcher("/data/box/../box/x", cfg, tmp_path) is not None


@pytest.mark.parametrize("kind,matcher", MATCHERS)
def test_home_dotdot_cannot_escape_allowed_tree(kind, matcher, tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    cfg = _config(kind, [Rule("allow", "~/bundle/MEMORY/**")])
    assert matcher("~/bundle/MEMORY/note.md", cfg, tmp_path) is not None
    assert matcher("~/bundle/MEMORY/../SOUL.md", cfg, tmp_path) is None


@pytest.mark.parametrize("kind,matcher", MATCHERS)
def test_dotdot_reaches_denied_path(kind, matcher, tmp_path):
    cfg = _config(kind, [Rule("allow", "/work/**"), Rule("deny", "/work/.dippy")])
    match = matcher("/work/tmp/../.dippy", cfg, tmp_path)
    assert match is not None and match.decision == "deny"


def test_remote_redirect_dotdot_cannot_escape_allowed_tree(tmp_path):
    cfg = Config(redirect_rules=[Rule("allow", "/tmp/**")])
    assert _match_redirect("/tmp/x", cfg, tmp_path, remote=True) is not None
    assert _match_redirect("/tmp/../etc/passwd", cfg, tmp_path, remote=True) is None


@pytest.mark.parametrize("kind,matcher", MATCHERS)
def test_leading_double_slash_reaches_deny_rule(kind, matcher, tmp_path):
    cfg = _config(kind, [Rule("allow", "/**"), Rule("deny", "/etc/**")])
    match = matcher("//etc/passwd", cfg, tmp_path)
    assert match is not None and match.decision == "deny"


def test_remote_redirect_leading_double_slash_reaches_deny_rule(tmp_path):
    cfg = Config(redirect_rules=[Rule("allow", "/**"), Rule("deny", "/etc/**")])
    match = _match_redirect("//etc/passwd", cfg, tmp_path, remote=True)
    assert match is not None and match.decision == "deny"


@pytest.mark.parametrize(
    "command",
    [
        "zorptool ~/proj/../.ssh/id_rsa",
        "zorptool /srv/proj/../../etc/shadow",
    ],
)
def test_command_rule_path_words_cannot_escape_with_dotdot(
    check, command, tmp_path, monkeypatch
):
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    cfg = parse_config("allow zorptool ~/proj/*\nallow zorptool /srv/proj/*\n")
    assert is_approved(check("zorptool ~/proj/notes.txt", cfg, tmp_path))
    assert is_approved(check("zorptool /srv/proj/notes.txt", cfg, tmp_path))
    assert needs_confirmation(check(command, cfg, tmp_path))
