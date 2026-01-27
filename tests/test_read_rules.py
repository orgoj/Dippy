"""Tests for read rules matching."""



from dippy.core.config import Config, Rule, match_read, parse_config


class TestMatchRead:
    """Test read file matching against config rules."""

    def test_basic_glob_match(self, tmp_path):
        cfg = Config(read_rules=[Rule("allow", "/tmp/*")])
        assert match_read("/tmp/foo", cfg, tmp_path) is not None
        assert match_read("/tmp/bar.txt", cfg, tmp_path) is not None
        assert match_read("/var/foo", cfg, tmp_path) is None

    def test_no_match_returns_none(self, tmp_path):
        cfg = Config(read_rules=[Rule("allow", "/tmp/*")])
        assert match_read("/var/log/test", cfg, tmp_path) is None

    def test_empty_rules_returns_none(self, tmp_path):
        cfg = Config(read_rules=[])
        assert match_read("/tmp/foo", cfg, tmp_path) is None

    def test_doublestar_matches_any_depth(self, tmp_path):
        cfg = Config(read_rules=[Rule("allow", "/tmp/**")])
        assert match_read("/tmp/foo", cfg, tmp_path) is not None
        assert match_read("/tmp/a/b/c", cfg, tmp_path) is not None
        assert match_read("/tmp/a/b/c/file.txt", cfg, tmp_path) is not None

    def test_relative_path_resolution(self, tmp_path):
        cfg = Config(read_rules=[Rule("allow", f"{tmp_path}/output.txt")])
        assert match_read("./output.txt", cfg, tmp_path) is not None
        assert match_read("output.txt", cfg, tmp_path) is not None

    def test_deny_read_basic(self, tmp_path):
        cfg = Config(read_rules=[Rule("deny", "/etc/**")])
        m = match_read("/etc/passwd", cfg, tmp_path)
        assert m is not None
        assert m.decision == "deny"

    def test_deny_read_with_message(self, tmp_path):
        cfg = Config(read_rules=[Rule("deny", "/etc/**", message="system files")])
        m = match_read("/etc/passwd", cfg, tmp_path)
        assert m.decision == "deny"
        assert m.message == "system files"

    def test_allow_read_override(self, tmp_path):
        # Last match wins
        cfg = Config(
            read_rules=[
                Rule("deny", "/etc/**"),
                Rule("allow", "/etc/hosts"),
            ]
        )
        m = match_read("/etc/hosts", cfg, tmp_path)
        assert m.decision == "allow"
        m2 = match_read("/etc/passwd", cfg, tmp_path)
        assert m2.decision == "deny"


class TestParseReadRules:
    """Test parsing of read rules from config text."""

    def test_allow_read(self):
        cfg = parse_config("allow-read /tmp/*")
        assert len(cfg.read_rules) == 1
        assert cfg.read_rules[0].decision == "allow"
        assert cfg.read_rules[0].pattern == "/tmp/*"

    def test_ask_read(self):
        cfg = parse_config('ask-read .env* "sensitive"')
        assert len(cfg.read_rules) == 1
        assert cfg.read_rules[0].decision == "ask"
        assert cfg.read_rules[0].pattern == ".env*"
        assert cfg.read_rules[0].message == "sensitive"

    def test_deny_read(self):
        cfg = parse_config("deny-read /etc/shadow")
        assert len(cfg.read_rules) == 1
        assert cfg.read_rules[0].decision == "deny"
        assert cfg.read_rules[0].pattern == "/etc/shadow"

    def test_mix_with_other_rules(self):
        cfg = parse_config("""
allow-read src/**
deny-edit src/locked.py
allow ls
""")
        assert len(cfg.read_rules) == 1
        assert cfg.read_rules[0].pattern == "src/**"
        assert len(cfg.edit_rules) == 1
        assert len(cfg.rules) == 1
