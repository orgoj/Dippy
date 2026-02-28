"""Tests for idle notifier configuration parsing."""

from dippy.core.config import parse_config, _merge_configs, Config


class TestIdleNotifierConfig:
    """Test idle notifier configuration parsing."""

    def test_parse_idle_notifier_command(self):
        """Test parsing idle-notifier-command setting."""
        text = "set idle-notifier-command ~/.dippy/notify-idle.sh"
        conf = parse_config(text)
        assert conf.idle_notifier_command is not None
        assert conf.idle_notifier_command.endswith("notify-idle.sh")

    def test_idle_notifier_command_none_by_default(self):
        """Test idle_notifier_command is None by default."""
        conf = parse_config("")
        assert conf.idle_notifier_command is None

    def test_command_as_template_with_placeholders(self):
        """Test idle-notifier-command can contain placeholders."""
        text = 'set idle-notifier-command notify-send "{title}" "{message}"'
        conf = parse_config(text)
        assert conf.idle_notifier_command == 'notify-send "{title}" "{message}"'

    def test_command_with_complex_template(self):
        """Test command with complex template."""
        text = (
            'set idle-notifier-command echo "{title} in {cwd}: {message}" >> /tmp/log'
        )
        conf = parse_config(text)
        assert "notify-send" not in conf.idle_notifier_command
        assert "echo" in conf.idle_notifier_command
        assert "{title}" in conf.idle_notifier_command
        assert "{cwd}" in conf.idle_notifier_command
        assert "{message}" in conf.idle_notifier_command

    def test_merge_idle_notifier_command(self):
        """Test merging idle_notifier_command."""
        base = Config(idle_notifier_command="/base.sh")
        overlay = Config(idle_notifier_command="/overlay.sh")
        merged = _merge_configs(base, overlay)
        assert merged.idle_notifier_command == "/overlay.sh"

    def test_merge_none_idle_notifier_command(self):
        """Test that None idle_notifier_command doesn't override set value."""
        base = Config(idle_notifier_command="/base.sh")
        overlay = Config(idle_notifier_command=None)
        merged = _merge_configs(base, overlay)
        assert merged.idle_notifier_command == "/base.sh"

    def test_full_config_with_idle_notifier(self):
        """Test complete config with idle notifier settings."""
        text = """
        allow ls
        set idle-notifier-command ~/.dippy/notify-idle.sh "{title}" "{message}" "{cwd}"
        """
        conf = parse_config(text)
        assert len(conf.rules) == 1
        assert conf.rules[0].decision == "allow"
        assert conf.idle_notifier_command is not None
        assert "{title}" in conf.idle_notifier_command
        assert "{message}" in conf.idle_notifier_command
        assert "{cwd}" in conf.idle_notifier_command
