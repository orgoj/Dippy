"""Tests for custom wrapper command system."""

from pathlib import Path

import pytest

from dippy.core.config import parse_config, Config
from dippy.core.analyzer import analyze, _extract_wrapper_args


class TestWrapperExtraction:
    """Test wrapper argument extraction logic."""

    def test_wrapper_extraction_basic(self):
        """Basic wrapper with destination and command."""
        dest, inner, _ = _extract_wrapper_args(["wrap", "server1", "free", "-h"])
        assert dest == "server1"
        assert inner == "free -h"

    def test_wrapper_extraction_quoted_command(self):
        """Wrapper with quoted command."""
        dest, inner, _ = _extract_wrapper_args(["wrap", "server1", '"free -h"'])
        assert dest == "server1"
        assert inner == '"free -h"'

    def test_wrapper_extraction_with_options(self):
        """Wrapper with options before destination."""
        dest, inner, _ = _extract_wrapper_args(
            ["wrap", "-p", "2222", "-l", "user", "server1", "ls"]
        )
        assert dest == "server1"
        assert inner == "ls"

    def test_wrapper_extraction_double_dash(self):
        """Wrapper with -- ending option parsing."""
        dest, inner, _ = _extract_wrapper_args(["wrap", "--", "server1", "ls"])
        assert dest == "server1"
        assert inner == "ls"

    def test_wrapper_extraction_no_inner_command(self):
        """Wrapper with destination but no inner command (interactive)."""
        dest, inner, _ = _extract_wrapper_args(["wrap", "server1"])
        assert dest == "server1"
        assert inner == ""

    def test_wrapper_extraction_no_destination(self):
        """Wrapper with no destination."""
        dest, inner, _ = _extract_wrapper_args(["wrap"])
        assert dest is None
        assert inner == ""

    def test_wrapper_extraction_only_options(self):
        """Wrapper with only options, no destination."""
        dest, inner, _ = _extract_wrapper_args(["wrap", "-p", "2222"])
        assert dest is None
        assert inner == ""


class TestConfigParser:
    """Test wrapper directive parsing."""

    def test_parse_single_wrapper(self):
        """Parse a single wrapper directive."""
        config = parse_config("wrapper wrap")
        assert "wrap" in config.wrappers
        assert config.wrappers["wrap"].name == "wrap"

    def test_parse_multiple_wrappers(self):
        """Parse multiple wrapper directives."""
        config = parse_config(
            """
            wrapper wrap
            wrapper ssh
            wrapper tmux-cli
        """
        )
        assert set(config.wrappers.keys()) == {"wrap", "ssh", "tmux-cli"}

    def test_parse_script_stdin_marker(self):
        """A wrapper can declare a marker for literal heredoc scripts."""
        config = parse_config("wrapper fictionalwrap --cmd run --script-stdin --script")

        assert config.wrappers["fictionalwrap"].script_stdin_marker == "--script"

    def test_duplicate_wrapper_warning(self, caplog):
        """Duplicate wrapper definition logs warning."""

        config = parse_config(
            """
            wrapper wrap
            wrapper wrap
        """
        )
        assert set(config.wrappers.keys()) == {"wrap"}
        assert "duplicate wrapper" in caplog.text.lower()

    def test_empty_wrapper_name_error(self, caplog):
        """Empty wrapper name logs error."""
        config = parse_config("wrapper")
        assert config.wrappers == {}
        assert any("requires a command name" in rec.message for rec in caplog.records)

    def test_wrapper_name_starting_with_dash_error(self, caplog):
        """Wrapper name starting with - logs error."""
        config = parse_config("wrapper -bad")
        assert config.wrappers == {}
        assert any("cannot start with '-'" in rec.message for rec in caplog.records)


class TestWrapperAnalysis:
    """Test wrapper command analysis."""

    def test_wrapper_with_inner_command_delegates(self):
        """Wrapper with inner command delegates to inner analysis."""
        config = parse_config(
            """
            wrapper wrap --context-first
            allow [wrap,server1] free *
        """
        )
        result = analyze("wrap server1 free -h", config, Path.cwd())
        assert result.action == "allow"

    def test_wrapper_context_flags_include_both(self):
        """Context flags include both wrapper name and destination."""
        config = parse_config(
            """
            wrapper wrap --context-first
            deny [wrap,server1] rm *
        """
        )
        result = analyze("wrap server1 rm /tmp/x", config, Path.cwd())
        assert result.action == "deny"
        assert "rm" in result.reason.lower()

    def test_wrapper_destination_only_flag(self):
        """Rule with only destination flag matches."""
        config = parse_config(
            """
            wrapper wrap --context-first
            allow [server1] free *
        """
        )
        result = analyze("wrap server1 free -h", config, Path.cwd())
        assert result.action == "allow"

    def test_wrapper_name_only_flag(self):
        """Rule with only wrapper name flag matches."""
        config = parse_config(
            """
            wrapper wrap
            allow [wrap] free *
        """
        )
        result = analyze("wrap server1 free -h", config, Path.cwd())
        assert result.action == "allow"

    def test_wrapper_negated_flag(self):
        """Negated wrapper flag works correctly."""
        config = parse_config(
            """
            wrapper wrap
            deny [!wrap] rm *
        """
        )
        # rm without wrap should be denied
        result = analyze("rm /tmp/x", config, Path.cwd())
        assert result.action == "deny"

        # rm with wrap should NOT match the negated rule (no rule = ask)
        result = analyze("wrap server1 rm /tmp/x", config, Path.cwd())
        assert result.action == "ask"  # No matching rule

    def test_wrapper_without_inner_command_asks(self):
        """Wrapper without inner command returns ask (interactive)."""
        config = parse_config("wrapper wrap")
        result = analyze("wrap server1", config, Path.cwd())
        assert result.action == "ask"
        assert "server1" in result.reason

    def test_wrapper_without_destination_asks(self):
        """Wrapper without destination returns ask."""
        config = parse_config("wrapper wrap")
        result = analyze("wrap", config, Path.cwd())
        assert result.action == "ask"

    def test_wrapper_with_options_delegates_correctly(self):
        """Wrapper with options skips them and extracts destination."""
        config = parse_config(
            """
            wrapper wrap --context-first
            allow [wrap,server1] ls *
        """
        )
        result = analyze("wrap -p 2222 server1 ls", config, Path.cwd())
        assert result.action == "allow"

    def test_generic_wrapper_with_subcommand_and_target(self):
        """Test wrapper with explicit trigger (run) and target flag (-t)."""
        config_text = """
            wrapper cca-tmux-cli --cmd run --flag -t --context-first
            allow [cca-tmux-cli,l2] ls *
            deny [cca-tmux-cli,prod] ls * "No ls on prod!"
        """
        config = parse_config(config_text)
        cwd = Path("/home/user")

        # 1. Allowed command on l2
        cmd_l2 = 'cca-tmux-cli -t l2 run "ls /data"'
        decision_l2 = analyze(cmd_l2, config, cwd)
        assert decision_l2.action == "allow"
        assert "ls" in decision_l2.reason

        # 2. Denied command on prod
        cmd_prod = 'cca-tmux-cli -t prod run "ls /data"'
        decision_prod = analyze(cmd_prod, config, cwd)
        assert decision_prod.action == "deny"
        assert "No ls on prod!" in decision_prod.reason

    def test_generic_wrapper_no_target_flag(self):
        """Test wrapper with trigger (exec) but no explicit target flag."""
        config_text = """
            wrapper mytool --cmd exec --context-first
            allow [mytool,myserver] id
        """
        config = parse_config(config_text)
        cwd = Path("/home/user")

        cmd = 'mytool myserver exec "id"'
        decision = analyze(cmd, config, cwd)
        assert decision.action == "allow"
        assert "id" in decision.reason

    def test_remote_relative_path_rule_matching(self):
        """Relative path rules in remote wrappers match literally without local cwd expansion."""
        config_text = """
            wrapper cca-tmux-cli --cmd run --flag -t --context-first
            allow [cca-tmux-cli,mp] ./node_modules/.bin/jest *
        """
        config = parse_config(config_text)
        cwd = Path("/home/michael/local_project")

        cmd = 'cca-tmux-cli -t mp run "./node_modules/.bin/jest --ci"'
        decision = analyze(cmd, config, cwd)
        assert decision.action == "allow"


class TestExistingWrappersStillWork:
    """Ensure existing ssh/sudo wrappers continue working."""

    def test_ssh_wrapper_context_still_works(self):
        """SSH handler still sets wrapper_context correctly."""
        config = parse_config("allow [ssh] free *")
        result = analyze("ssh host free -h", config, Path.cwd())
        assert result.action == "allow"

    def test_sudo_wrapper_context_still_works(self):
        """Sudo handler still sets wrapper_context correctly."""
        config = parse_config("allow [sudo] free *")
        result = analyze("sudo free -h", config, Path.cwd())
        assert result.action == "allow"

    def test_ssh_and_custom_wrapper_coexist(self):
        """SSH and custom wrappers can both be defined."""
        config = parse_config(
            """
            wrapper wrap --context-first
            allow [ssh] free *
            allow [wrap,server1] ls *
        """
        )
        # SSH should work
        result = analyze("ssh host free -h", config, Path.cwd())
        assert result.action == "allow"

        # Custom wrapper should also work
        result = analyze("wrap server1 ls", config, Path.cwd())
        assert result.action == "allow"


class TestWrapperConfigMerge:
    """Test wrapper merging across config scopes."""

    def test_wrappers_merge_with_union(self):
        """Wrappers from multiple configs merge via set union."""
        from dippy.core.config import _merge_configs, WrapperInfo

        base = Config(wrappers={"wrap1": WrapperInfo(name="wrap1")})
        overlay = Config(wrappers={"wrap2": WrapperInfo(name="wrap2")})
        merged = _merge_configs(base, overlay)

        assert set(merged.wrappers.keys()) == {"wrap1", "wrap2"}

    def test_duplicate_wrappers_deduplicate(self):
        """Duplicate wrapper names are deduplicated in merge."""
        from dippy.core.config import _merge_configs, WrapperInfo

        base = Config(wrappers={"wrap": WrapperInfo(name="wrap", trigger="old")})
        overlay = Config(wrappers={"wrap": WrapperInfo(name="wrap", trigger="new")})
        merged = _merge_configs(base, overlay)

        assert set(merged.wrappers.keys()) == {"wrap"}
        assert merged.wrappers["wrap"].trigger == "new"  # Overlay wins


class TestWrapperScriptStdin:
    """Quoted heredoc wrapper mode analyzes the literal remote script."""

    @staticmethod
    def config():
        return parse_config(
            """
            wrapper fictionalwrap --cmd run --context -t --script-stdin --script
            allow [fictionalwrap] fictionalread *
            allow [fictionalwrap] fictionalpython *
            ask [fictionalwrap] fictionalwrite *
            """
        )

    def test_safe_multiline_script_is_allowed(self):
        result = analyze(
            """fictionalwrap -t server1 run --script <<'REMOTE'
fictionalread /one
fictionalread /two
REMOTE""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "allow"

    def test_unsafe_command_in_script_asks(self):
        result = analyze(
            """fictionalwrap -t server1 run --script <<'REMOTE'
fictionalread /one
fictionalwrite /two
REMOTE""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "ask"

    def test_nested_quoted_heredoc_is_script_data(self):
        result = analyze(
            """fictionalwrap -t server1 run --script <<'REMOTE'
fictionalpython - <<'PY'
print("literal $HOME and `fictionalwrite /hidden`")
PY
REMOTE""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "allow"

    def test_wrapper_options_before_script_marker_allowed(self):
        result = analyze(
            """fictionalwrap -t server1 run --timeout 3 --script <<'REMOTE'
fictionalread /one
REMOTE""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "allow"

    def test_multiple_wrapper_options_before_script_marker_allowed(self):
        result = analyze(
            """fictionalwrap -t server1 run --timeout 600 --interval 2 --script <<'REMOTE'
fictionalread /one
REMOTE""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "allow"

    def test_wrapper_options_with_expansion_asks(self):
        result = analyze(
            """fictionalwrap -t server1 run --timeout $(fictionalread /one) --script <<'REMOTE'
fictionalread /one
REMOTE""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "ask"

    def test_wrapper_positional_command_before_script_marker_asks(self):
        result = analyze(
            """fictionalwrap -t server1 run extra_cmd --script <<'REMOTE'
fictionalread /one
REMOTE""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "ask"

    def test_unquoted_heredoc_asks(self):
        result = analyze(
            """fictionalwrap -t server1 run --script <<REMOTE
fictionalread /one
REMOTE""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "ask"

    def test_empty_heredoc_asks(self):
        result = analyze(
            """fictionalwrap -t server1 run --script <<'REMOTE'
REMOTE""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "ask"

    def test_file_input_asks(self):
        result = analyze(
            "fictionalwrap -t server1 run --script < payload.sh",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "ask"

    def test_pipeline_input_asks(self):
        result = analyze(
            "printf payload | fictionalwrap -t server1 run --script",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "ask"

    def test_inline_payload_with_heredoc_asks(self):
        result = analyze(
            """fictionalwrap -t server1 run --script extra <<'REMOTE'
fictionalread /one
REMOTE""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "ask"

    def test_additional_redirect_asks(self):
        result = analyze(
            """fictionalwrap -t server1 run --script > output <<'REMOTE'
fictionalread /one
REMOTE""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "ask"

    def test_legacy_string_mode_is_unchanged(self):
        result = analyze(
            'fictionalwrap -t server1 run "fictionalread /one"',
            self.config(),
            Path.cwd(),
        )

        assert result.action == "allow"


class TestDippyScriptStdin:
    """Direct execution accepts only literal heredoc scripts for delegation."""

    @staticmethod
    def config():
        return parse_config(
            """
            allow fictionalread *
            ask fictionalerase *
            allow [run-on-server,server1] fictionalremote *
            """
        )

    def test_run_analyzes_quoted_heredoc_locally(self):
        result = analyze(
            """dippy run <<'DIPPY'
fictionalread /one
fictionalread /two
DIPPY""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "allow"

    def test_run_on_server_analyzes_quoted_heredoc_remotely(self):
        result = analyze(
            """dippy run-on-server server1 <<'DIPPY'
fictionalremote /one
DIPPY""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "allow"
        assert {"run-on-server", "server1"} <= result.context_flags

    def test_destructive_command_in_script_asks(self):
        result = analyze(
            """dippy run <<'DIPPY'
fictionalread /one
fictionalerase --all
DIPPY""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "ask"

    @pytest.mark.parametrize("prefix", ["", "MODE=test "])
    def test_expanded_server_asks(self, prefix):
        result = analyze(
            f"""{prefix}dippy run-on-server "$SERVER" <<'DIPPY'
fictionalremote /one
DIPPY""",
            self.config(),
            Path.cwd(),
        )

        assert result.action == "ask"

    @pytest.mark.parametrize(
        "command",
        [
            """dippy run <<DIPPY
fictionalread /one
DIPPY""",
            """dippy run extra <<'DIPPY'
fictionalread /one
DIPPY""",
            """dippy run > output <<'DIPPY'
fictionalread /one
DIPPY""",
            """dippy run <<'DIPPY'
DIPPY""",
        ],
    )
    def test_noncanonical_input_asks(self, command):
        assert analyze(command, self.config(), Path.cwd()).action == "ask"

    def test_quoted_argument_mode_is_unchanged(self):
        result = analyze("dippy run 'fictionalread /one'", self.config(), Path.cwd())

        assert result.action == "ask"


class TestScriptOutputRedirects:
    @pytest.fixture
    def config(self, tmp_path, monkeypatch):
        monkeypatch.setenv("HOME", str(tmp_path))
        return parse_config(
            """
            wrapper fictionalwrap --cmd run --context -t --script-stdin --script
            allow fictionalread *
            ask fictionalerase *
            allow-redirect tmp/**
            deny-redirect protected/**
            """
        )

    @pytest.fixture(
        params=[
            "fictionalwrap -t server1 run --script",
            "fictionalwrap -t server1 run --timeout 600 --script",
            "dippy run",
            "dippy run-on-server server1",
        ]
    )
    def invocation(self, request):
        return request.param

    @pytest.mark.parametrize(
        "redirect",
        [
            "> tmp/output.log",
            ">> tmp/output.log",
            "2> tmp/error.log",
            "2>> tmp/error.log",
            "&> tmp/output.log",
            "&>> tmp/output.log",
            ">| tmp/output.log",
            "> tmp/output.log 2>&1",
        ],
    )
    @pytest.mark.parametrize("before", [True, False])
    def test_allowed_output(self, config, invocation, redirect, before, tmp_path):
        suffix = f"{redirect} <<'REMOTE'" if before else f"<<'REMOTE' {redirect}"
        result = analyze(
            f"{invocation} {suffix}\nfictionalread /one\nREMOTE",
            config,
            tmp_path,
        )
        assert result.action == "allow"

    @pytest.mark.parametrize(
        ("redirect", "action"),
        [("> unknown/output.log", "ask"), ("> protected/output.log", "deny")],
    )
    def test_output_policy(self, config, invocation, redirect, action, tmp_path):
        result = analyze(
            f"{invocation} {redirect} <<'REMOTE'\nfictionalread /one\nREMOTE",
            config,
            tmp_path,
        )
        assert result.action == action

    def test_unsafe_body_with_allowed_output(self, config, invocation, tmp_path):
        result = analyze(
            f"{invocation} > tmp/output.log <<'REMOTE'\nfictionalerase --all\nREMOTE",
            config,
            tmp_path,
        )
        assert result.action == "ask"

    @pytest.mark.parametrize(
        "redirect",
        [
            "< payload.sh",
            "<<< 'fictionalerase --all'",
            "0<&3",
            "0> tmp/output.log",
            "0>&3",
            "<> tmp/output.log",
            "{fd}> tmp/output.log",
            "2>&0-",
            "2>&00-",
            "2>&$FD",
        ],
    )
    @pytest.mark.parametrize("before", [True, False])
    def test_input_or_ambiguous_redirect_asks(
        self, config, invocation, redirect, before, tmp_path
    ):
        suffix = f"{redirect} <<'REMOTE'" if before else f"<<'REMOTE' {redirect}"
        result = analyze(
            f"{invocation} {suffix}\nfictionalread /one\nREMOTE",
            config,
            tmp_path,
        )
        assert result.action == "ask"

    def test_non_stdin_heredoc_asks(self, config, invocation, tmp_path):
        result = analyze(
            f"{invocation} 3<<'REMOTE'\nfictionalread /one\nREMOTE",
            config,
            tmp_path,
        )
        assert result.action == "ask"

    def test_multiple_heredocs_ask(self, config, invocation, tmp_path):
        result = analyze(
            f"{invocation} <<'ONE' <<'TWO'\nfictionalread /one\nONE\n"
            "fictionalerase --all\nTWO",
            config,
            tmp_path,
        )
        assert result.action == "ask"

    @pytest.mark.parametrize(
        "heredoc",
        ["<<REMOTE\nfictionalread /one\nREMOTE", "<<'REMOTE'\nREMOTE"],
    )
    def test_invalid_heredoc_with_output_asks(
        self, config, invocation, heredoc, tmp_path
    ):
        result = analyze(f"{invocation} > tmp/output.log {heredoc}", config, tmp_path)
        assert result.action == "ask"

    def test_output_expansion_is_checked(self, config, invocation, tmp_path):
        result = analyze(
            f"{invocation} > tmp/$(fictionalerase --all) <<'REMOTE'\n"
            "fictionalread /one\nREMOTE",
            config,
            tmp_path,
        )
        assert result.action == "ask"


class TestLnavWrapperValidation:
    """Test run-on-server wrapper with lnav command validation."""

    def test_lnav_correct_format_allowed(self):
        """Correct lnav format with -n -c SQL -c :write-table-to is allowed."""
        config = parse_config(
            """
            wrapper run-on-server
            deny [run-on-server] lnav * "Use exact format: lnav -n -c SQL -c :write-table-to - /log/file.log"
            allow [run-on-server] lnav -n -c ;* -c :write-table-to -* /log/**log
        """
        )
        cmd = 'run-on-server ferda7 \'lnav -n -c ";SELECT col" -c ":write-table-to -" /log/file.log\''
        result = analyze(cmd, config, Path.cwd())
        assert result.action == "allow"

    def test_lnav_missing_semicolon_denied(self):
        """lnav without semicolon prefix in SQL is denied."""
        config = parse_config(
            """
            wrapper run-on-server
            deny [run-on-server] lnav * "Use exact format: lnav -n -c SQL -c :write-table-to - /log/file.log"
            allow [run-on-server] lnav -n -c ;* -c :write-table-to -* /log/**log
        """
        )
        cmd = 'run-on-server ferda7 \'lnav -n -c "SELECT col" -c ":write-table-to -" /log/file.log\''
        result = analyze(cmd, config, Path.cwd())
        assert result.action == "deny"

    def test_lnav_missing_n_flag_denied(self):
        """lnav without -n flag is denied."""
        config = parse_config(
            """
            wrapper run-on-server
            deny [run-on-server] lnav * "Use exact format: lnav -n -c SQL -c :write-table-to - /log/file.log"
            allow [run-on-server] lnav -n -c ;* -c :write-table-to -* /log/**log
        """
        )
        cmd = 'run-on-server ferda7 \'lnav -c ";SELECT col" -c ":write-table-to -" /log/file.log\''
        result = analyze(cmd, config, Path.cwd())
        assert result.action == "deny"

    def test_lnav_missing_second_c_denied(self):
        """lnav without second -c (write-table-to) is denied."""
        config = parse_config(
            """
            wrapper run-on-server
            deny [run-on-server] lnav * "Use exact format: lnav -n -c SQL -c :write-table-to - /log/file.log"
            allow [run-on-server] lnav -n -c ;* -c :write-table-to -* /log/**log
        """
        )
        cmd = "run-on-server ferda7 'lnav -n -c \";SELECT col\" /log/file.log'"
        result = analyze(cmd, config, Path.cwd())
        assert result.action == "deny"

    def test_lnav_wrong_path_denied(self):
        """lnav with path not matching /log/**log is denied."""
        config = parse_config(
            """
            wrapper run-on-server
            deny [run-on-server] lnav * "Use exact format: lnav -n -c SQL -c :write-table-to - /log/file.log"
            allow [run-on-server] lnav -n -c ;* -c :write-table-to -* /log/**log
        """
        )
        cmd = 'run-on-server ferda7 \'lnav -n -c ";SELECT col" -c ":write-table-to -" /var/log/file.log\''
        result = analyze(cmd, config, Path.cwd())
        assert result.action == "deny"
