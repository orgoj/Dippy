"""Tests for transparent wrapper commands (timeout, nohup, nice, ionice, etc.)."""

from pathlib import Path

from dippy.core.config import parse_config
from dippy.core.analyzer import analyze


class TestTransparentWrappers:
    """Test transparent wrappers unwrapping to inner commands."""

    def test_timeout_with_unit_suffix(self):
        """timeout 500s or 10m should unwrap to inner command."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("timeout 500s fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_timeout_with_flags(self):
        """timeout -s KILL 10s should unwrap to inner command."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("timeout -s KILL 10s fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_timeout_with_kill_after(self):
        """timeout -k 5s 10s should unwrap to inner command."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("timeout -k 5s 10s fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_ionice_unwraps(self):
        """ionice should unwrap to inner command."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("ionice -c 3 fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_ionice_combined_args(self):
        """ionice -c3 -n7 should unwrap to inner command."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("ionice -c3 -n7 fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_stacked_wrappers(self):
        """nohup nice -n 19 ionice -c3 should unwrap all layers."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze(
            "nohup nice -n 19 ionice -c3 fictional_runner test", config, Path.cwd()
        )
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_timeout_55s_ionice_nice_stacked(self):
        """timeout 55s ionice -c3 nice -n 19 should unwrap all layers."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze(
            "timeout 55s ionice -c3 nice -n 19 fictional_runner test",
            config,
            Path.cwd(),
        )
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_unwrapping_before_context_catch_all(self):
        """Wrapper must unwrap before catch-all ask rule in context."""
        config_text = """
wrapper cca-tmux-cli --cmd run --context -t --script-stdin --script
ask [cca-tmux-cli] * "Not in remote allowlist."
allow [cca-tmux-cli] fictional_runner *
"""
        config = parse_config(config_text, Path.cwd())
        cmd = """cca-tmux-cli -t mpaheca run --script <<'REMOTE'
cd /tmp && timeout 500 fictional_runner --ci
REMOTE"""
        dec = analyze(cmd, config, Path.cwd())
        assert dec.action == "allow"

    def test_unwrapping_stacked_before_context_catch_all(self):
        """Stacked nohup nice ionice must unwrap before catch-all ask rule."""
        config_text = """
wrapper cca-tmux-cli --cmd run --context -t --script-stdin --script
ask [cca-tmux-cli] * "Not in remote allowlist."
allow [cca-tmux-cli] fictional_runner *
"""
        config = parse_config(config_text, Path.cwd())
        cmd = """cca-tmux-cli -t mpaheca run --script <<'REMOTE'
nohup nice -n 19 ionice -c3 fictional_runner --ci
REMOTE"""
        dec = analyze(cmd, config, Path.cwd())
        assert dec.action == "allow"

    def test_unallowed_inner_command_still_asks(self):
        """Unallowed command inside wrapper must ask, naming inner command."""
        config = parse_config("", Path.cwd())
        dec = analyze("timeout 10s fictional_unknown_cmd arg", config, Path.cwd())
        assert dec.action == "ask"
        assert "fictional_unknown_cmd" in dec.reason

    def test_destructive_inner_command_not_allowed(self):
        """Destructive command inside wrapper must not be allowed."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("timeout 10s rm -rf /", config, Path.cwd())
        assert dec.action != "allow"

    def test_custom_transparent_wrapper_config(self):
        """wrapper <name> --transparent should unwrap custom wrapper."""
        config_text = """
wrapper custom_runner --transparent
allow fictional_runner *
"""
        config = parse_config(config_text, Path.cwd())
        dec = analyze("custom_runner fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_custom_transparent_wrapper_with_options(self):
        """wrapper <name> --transparent should skip options before inner command."""
        config_text = """
wrapper custom_runner --transparent
allow fictional_runner *
"""
        config = parse_config(config_text, Path.cwd())
        dec = analyze(
            "custom_runner -v --dry-run fictional_runner test", config, Path.cwd()
        )
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_taskset_unwraps(self):
        """taskset -c 0,1 should unwrap to inner command."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("taskset -c 0,1 fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_taskset_hex_mask_unwraps(self):
        """taskset 0x1 should unwrap to inner command."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("taskset 0x1 fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_chrt_unwraps(self):
        """chrt -f 10 should unwrap to inner command."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("chrt -f 10 fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_stdbuf_unwraps(self):
        """stdbuf -oL -eL should unwrap to inner command."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("stdbuf -oL -eL fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_flock_unwraps(self):
        """flock /tmp/lockfile should unwrap to inner command."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("flock -n /tmp/my.lock fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_time_unwraps(self):
        """time -p should unwrap to inner command."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("time -p fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_nice_obsolete_syntax_unwraps(self):
        """nice -19 should unwrap to inner command."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("nice -19 fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_command_v_still_allowed(self):
        """command -v should remain allowed directly."""
        config = parse_config("", Path.cwd())
        dec = analyze("command -v fictional_runner", config, Path.cwd())
        assert dec.action == "allow"
        assert dec.reason == "command -v"

    def test_command_wrapper_unwraps(self):
        """command fictional_runner should unwrap to fictional_runner."""
        config = parse_config("allow fictional_runner *", Path.cwd())
        dec = analyze("command fictional_runner test", config, Path.cwd())
        assert dec.action == "allow"
        assert "fictional_runner" in dec.reason

    def test_explicit_deny_on_wrapper_prevails(self):
        """Explicit deny rule on wrapper command must not be bypassed by unwrapping."""
        config_text = """
deny timeout * "timeout forbidden"
allow fictional_runner *
"""
        config = parse_config(config_text, Path.cwd())
        dec = analyze("timeout 10 fictional_runner test", config, Path.cwd())
        assert dec.action == "deny"
        assert "timeout forbidden" in dec.reason
