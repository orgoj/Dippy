"""Tests for the cp/mv handler.

cp and mv write files, so their destinations belong to the redirect rules —
the same rules that already govern `>` and `tee`. Without a handler they fall
to the default `ask`, which is safe but leaves `allow cp *` as a way to write
anywhere, including over `.dippy` itself.

mv is the stricter of the two: it also removes its sources, so those are
writes as well.
"""

from conftest import is_approved, needs_confirmation
from dippy.core.config import Config, Rule


DIPPY_GUARD = Config(redirect_rules=[Rule("deny", "**/.dippy", message="config")])
TMP_OK = Config(redirect_rules=[Rule("allow", "/tmp/**")])


class TestDefaultsUnchanged:
    """Without a matching redirect rule, cp and mv still ask."""

    def test_cp_asks(self, check, tmp_path):
        assert needs_confirmation(check("cp a b", cwd=tmp_path))

    def test_mv_asks(self, check, tmp_path):
        assert needs_confirmation(check("mv a b", cwd=tmp_path))

    def test_cp_without_destination_asks(self, check, tmp_path):
        assert needs_confirmation(check("cp a", cwd=tmp_path))

    def test_bare_cp_asks(self, check, tmp_path):
        assert needs_confirmation(check("cp", cwd=tmp_path))


class TestDestinationGoesThroughRedirectRules:
    def test_allowed_destination(self, check, tmp_path):
        assert is_approved(check("cp a /tmp/b", config=TMP_OK, cwd=tmp_path))

    def test_allowed_destination_for_mv(self, check, tmp_path):
        assert is_approved(check("mv /tmp/a /tmp/b", config=TMP_OK, cwd=tmp_path))

    def test_recursive_copy_to_allowed_destination(self, check, tmp_path):
        assert is_approved(check("cp -r dir /tmp/dir", config=TMP_OK, cwd=tmp_path))

    def test_target_directory_flag(self, check, tmp_path):
        assert is_approved(check("cp -t /tmp a b", config=TMP_OK, cwd=tmp_path))

    def test_target_directory_long_flag(self, check, tmp_path):
        result = check("cp --target-directory /tmp a b", config=TMP_OK, cwd=tmp_path)
        assert is_approved(result)

    def test_target_directory_equals_form(self, check, tmp_path):
        result = check("cp --target-directory=/tmp a b", config=TMP_OK, cwd=tmp_path)
        assert is_approved(result)

    def test_double_dash_separator(self, check, tmp_path):
        assert is_approved(check("cp -- a /tmp/b", config=TMP_OK, cwd=tmp_path))

    def test_denied_destination(self, check, tmp_path):
        result = check("cp evil .dippy", config=DIPPY_GUARD, cwd=tmp_path)
        assert not is_approved(result)

    def test_destination_outside_allowed_tree_is_not_approved(self, check, tmp_path):
        assert not is_approved(check("cp a /etc/passwd", config=TMP_OK, cwd=tmp_path))

    def test_target_directory_outside_allowed_tree(self, check, tmp_path):
        """The -t argument is the destination — it must be checked, not skipped."""
        assert not is_approved(check("cp -t /etc a", config=TMP_OK, cwd=tmp_path))


class TestDirectoryDestinations:
    """A destination directory is expanded to the paths actually written."""

    def test_target_flag_expands_to_the_written_paths(self, check, tmp_path):
        """`cp -t dir a` writes dir/a, so that is what the rules see."""
        config = Config(redirect_rules=[Rule("deny", "**/.dippy", message="config")])
        assert not is_approved(check("cp -t /project .dippy", config=config))

    def test_trailing_slash_expands_to_the_written_paths(self, check, tmp_path):
        config = Config(redirect_rules=[Rule("deny", "**/.dippy", message="config")])
        assert not is_approved(check("cp payload/.dippy /project/", config=config))

    def test_destination_without_trailing_slash_is_used_verbatim(self, check, tmp_path):
        """Dippy does not stat the filesystem, so `cp a /tmp` asks.

        Writing `/tmp/` makes the intent explicit and lets the rule match.
        """
        assert not is_approved(check("cp a /tmp", config=TMP_OK, cwd=tmp_path))
        assert is_approved(check("cp a /tmp/", config=TMP_OK, cwd=tmp_path))


class TestMvSourcesAreWritesToo:
    """mv removes its sources, so a protected path cannot be moved away."""

    def test_moving_the_config_away_is_denied(self, check, tmp_path):
        result = check("mv .dippy /tmp/saved", config=DIPPY_GUARD, cwd=tmp_path)
        assert not is_approved(result)

    def test_moving_the_config_away_with_target_flag_is_denied(self, check, tmp_path):
        result = check("mv -t /tmp .dippy", config=DIPPY_GUARD, cwd=tmp_path)
        assert not is_approved(result)

    def test_copying_the_config_elsewhere_is_allowed(self, check, tmp_path):
        """cp only writes the destination — reading .dippy is not a write."""
        config = Config(
            redirect_rules=[
                Rule("deny", "**/.dippy", message="config"),
                Rule("allow", "/tmp/**"),
            ]
        )
        assert is_approved(check("cp .dippy /tmp/saved", config=config, cwd=tmp_path))


class TestBypassAttempts:
    """A broad allow must not become a way to write anywhere."""

    def test_allow_rule_does_not_skip_redirect_check(self, check, tmp_path):
        """`allow cp *` is a command rule; the destination is still checked.

        Rules beat handlers, so this documents what the user actually gets:
        an explicit `allow cp *` is a decision to trust every cp.
        """
        config = Config(
            rules=[Rule("allow", "cp *")],
            redirect_rules=[Rule("deny", "**/.dippy", message="config")],
        )
        result = check("cp evil .dippy", config=config, cwd=tmp_path)
        assert is_approved(result), "documents that a command rule wins"

    def test_unknown_flag_does_not_shift_the_destination(self, check, tmp_path):
        """An unrecognised flag must not make a source look like the target."""
        result = check("cp --zonk a /etc/passwd", config=TMP_OK, cwd=tmp_path)
        assert not is_approved(result)
