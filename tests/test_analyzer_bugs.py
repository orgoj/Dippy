"""Tests for analyzer bugs found in PR #29 review."""

from pathlib import Path

import pytest

from dippy.core.analyzer import analyze
from dippy.core.config import Config


class TestEnvVarPrefixHandling:
    """Handler should receive tokens without env var prefixes."""

    @pytest.fixture
    def config(self):
        return Config()

    @pytest.fixture
    def cwd(self):
        return Path.cwd()

    def test_git_status_with_env_var(self, config, cwd):
        """FOO=bar git status should be recognized as 'git status'."""
        result = analyze("FOO=bar git status", config, cwd)
        # Should recognize this as git status (safe read operation)
        assert result.action == "allow"
        assert result.reason == "git status"

    def test_git_log_with_multiple_env_vars(self, config, cwd):
        """Multiple env vars should all be skipped."""
        result = analyze("FOO=bar BAZ=qux git log", config, cwd)
        assert result.action == "allow"
        assert result.reason == "git log"

    def test_docker_ps_with_env_var(self, config, cwd):
        """DOCKER_HOST=x docker ps should work."""
        result = analyze("DOCKER_HOST=tcp://localhost:2375 docker ps", config, cwd)
        assert result.action == "allow"
        assert result.reason == "docker ps"

    def test_env_var_with_unsafe_command(self, config, cwd):
        """Env var prefix shouldn't hide unsafe commands."""
        result = analyze("FOO=bar git push", config, cwd)
        assert result.action == "ask"
        assert result.reason == "git push"


class TestCmdsubInjectionWarning:
    """Pure cmdsubs in handler CLIs should warn about injection risk."""

    @pytest.fixture
    def config(self):
        return Config()

    @pytest.fixture
    def cwd(self):
        return Path.cwd()

    def test_git_cmdsub_injection_reason(self, config, cwd):
        """git $(echo status) should mention injection risk."""
        result = analyze("git $(echo status)", config, cwd)
        assert result.action == "ask"
        assert "injection" in result.reason.lower()

    def test_docker_cmdsub_injection_reason(self, config, cwd):
        """docker $(echo run) should mention injection risk."""
        result = analyze("docker $(echo run) alpine", config, cwd)
        assert result.action == "ask"
        assert "injection" in result.reason.lower()

    def test_kubectl_cmdsub_injection_reason(self, config, cwd):
        """kubectl $(echo delete) should mention injection risk."""
        result = analyze("kubectl $(echo delete) pod foo", config, cwd)
        assert result.action == "ask"
        assert "injection" in result.reason.lower()


class TestNegationAndArith:
    """Test negation (!) and arithmetic (( )) constructs."""

    @pytest.fixture
    def config(self):
        return Config()

    @pytest.fixture
    def cwd(self):
        return Path.cwd()

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            ("! grep foo", "allow"),
            ("! rm file", "ask"),
            ("(( i++ ))", "allow"),
            ("(( x = 5 ))", "allow"),
            ("(( x = $(echo 1) ))", "allow"),  # safe cmdsub
            ("(( arr[$(rm -rf /)] ))", "ask"),  # dangerous cmdsub in subscript
        ],
    )
    def test_negation_and_arith(self, cmd, expected, config, cwd):
        assert analyze(cmd, config, cwd).action == expected


class TestCoproc:
    """Test coproc construct."""

    @pytest.fixture
    def config(self):
        return Config()

    @pytest.fixture
    def cwd(self):
        return Path.cwd()

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            ("coproc cat", "allow"),
            ("coproc { echo hi; }", "allow"),
            ("coproc NAME { echo hi; }", "allow"),
            ("coproc NAME { cat; }", "allow"),
            ("coproc rm -rf /", "ask"),
            ("coproc { rm -rf /; }", "ask"),
            ("coproc NAME { rm file; }", "ask"),
        ],
    )
    def test_coproc(self, cmd, expected, config, cwd):
        assert analyze(cmd, config, cwd).action == expected


class TestCondExpr:
    """Test [[ ]] conditional expression construct."""

    @pytest.fixture
    def config(self):
        return Config()

    @pytest.fixture
    def cwd(self):
        return Path.cwd()

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            # Simple conditions - allow
            ("[[ -f foo ]]", "allow"),
            ('[[ -z "$x" ]]', "allow"),
            ("[[ $a == $b ]]", "allow"),
            ("[[ -f x && -d y ]]", "allow"),
            ("[[ -f x || -d y ]]", "allow"),
            ("[[ ! -f foo ]]", "allow"),
            ("[[ ( -f x ) ]]", "allow"),
            # Safe cmdsubs - allow
            ("[[ -f $(echo foo) ]]", "allow"),
            ("[[ $(echo x) == y ]]", "allow"),
            ("[[ -f x && $(pwd) == y ]]", "allow"),
            # Dangerous cmdsubs - ask
            ("[[ -f $(rm -rf /) ]]", "ask"),
            ("[[ $(rm file) == x ]]", "ask"),
            ("[[ -f x && $(rm y) == z ]]", "ask"),
            ("[[ ! -f $(rm foo) ]]", "ask"),
            ("[[ ( $(rm x) == y ) ]]", "ask"),
        ],
    )
    def test_cond_expr(self, cmd, expected, config, cwd):
        assert analyze(cmd, config, cwd).action == expected


class TestCmdsubSecurityGaps:
    """Tests for cmdsub analysis in various constructs.

    These tests verify that dangerous command substitutions are detected
    in all contexts, not just simple command arguments.
    """

    @pytest.fixture
    def config(self):
        return Config()

    @pytest.fixture
    def cwd(self):
        return Path.cwd()

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            # for loop iteration words
            ("for i in $(rm foo); do echo $i; done", "ask"),
            ("for i in $(ls); do echo $i; done", "allow"),
            ("for i in a $(rm foo) b; do echo $i; done", "ask"),
        ],
    )
    def test_for_iteration_cmdsub(self, cmd, expected, config, cwd):
        """Cmdsubs in for loop iteration list should be analyzed."""
        assert analyze(cmd, config, cwd).action == expected

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            # select word list
            ("select x in $(rm foo); do echo $x; done", "ask"),
            ("select x in $(ls); do echo $x; done", "allow"),
        ],
    )
    def test_select_words_cmdsub(self, cmd, expected, config, cwd):
        """Cmdsubs in select word list should be analyzed."""
        assert analyze(cmd, config, cwd).action == expected

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            # case word
            ("case $(rm foo) in *) echo y;; esac", "ask"),
            ("case $(echo x) in *) echo y;; esac", "allow"),
        ],
    )
    def test_case_word_cmdsub(self, cmd, expected, config, cwd):
        """Cmdsubs in case word should be analyzed."""
        assert analyze(cmd, config, cwd).action == expected

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            # subshell with redirect containing cmdsub
            ("(ls) > $(rm foo)", "ask"),
            ("(ls) > $(echo /tmp/out)", "ask"),  # still ask - output redirect
            # brace-group with redirect containing cmdsub
            ("{ ls; } > $(rm foo)", "ask"),
            ("{ ls; } > $(echo /tmp/out)", "ask"),  # still ask - output redirect
        ],
    )
    def test_compound_redirect_cmdsub(self, cmd, expected, config, cwd):
        """Cmdsubs in redirect targets of compound commands should be analyzed."""
        assert analyze(cmd, config, cwd).action == expected

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            # Redirect target with cmdsub - inner command should be analyzed
            ("ls > $(rm foo)", "ask"),
            # Even safe inner cmdsub should ask due to output redirect
            ("ls > $(echo /tmp/out)", "ask"),
        ],
    )
    def test_redirect_target_cmdsub(self, cmd, expected, config, cwd):
        """Cmdsubs in redirect targets should be analyzed."""
        assert analyze(cmd, config, cwd).action == expected


class TestArithCmdRedirect:
    """Tests for arith-cmd redirect checking."""

    @pytest.fixture
    def config(self):
        return Config()

    @pytest.fixture
    def cwd(self):
        return Path.cwd()

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            ("(( 1 )) > $(rm foo)", "ask"),
            ("(( x++ )) > /tmp/out", "ask"),
        ],
    )
    def test_arith_cmd_redirect(self, cmd, expected, config, cwd):
        """Arith-cmd should check its redirects."""
        assert analyze(cmd, config, cwd).action == expected


class TestForArithCmdsub:
    """Tests for cmdsubs in for-arith init/cond/incr expressions."""

    @pytest.fixture
    def config(self):
        return Config()

    @pytest.fixture
    def cwd(self):
        return Path.cwd()

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            ("for (( i=$(rm foo); i<10; i++ )); do echo $i; done", "ask"),
            ("for (( i=0; i<$(rm foo); i++ )); do echo $i; done", "ask"),
            ("for (( i=0; i<10; i+=$(rm foo) )); do echo $i; done", "ask"),
        ],
    )
    def test_for_arith_cmdsub(self, cmd, expected, config, cwd):
        """Cmdsubs in for-arith init/cond/incr should be analyzed."""
        assert analyze(cmd, config, cwd).action == expected


class TestParamExpansionCmdsub:
    """Tests for cmdsubs nested inside parameter expansions."""

    @pytest.fixture
    def config(self):
        return Config()

    @pytest.fixture
    def cwd(self):
        return Path.cwd()

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            ("echo ${x:-$(rm foo)}", "ask"),
            ("echo ${x:=$(rm foo)}", "ask"),
            ("echo ${x:+$(rm foo)}", "ask"),
            ("echo ${x:?$(rm foo)}", "ask"),
            ("[[ -f ${x:-$(rm foo)} ]]", "ask"),
            ("for i in ${x:-$(rm foo)}; do echo $i; done", "ask"),
        ],
    )
    def test_param_expansion_cmdsub(self, cmd, expected, config, cwd):
        """Cmdsubs nested in parameter expansions should be analyzed."""
        assert analyze(cmd, config, cwd).action == expected


class TestBacktickCmdsub:
    """Tests for backtick command substitutions in raw strings."""

    @pytest.fixture
    def config(self):
        return Config()

    @pytest.fixture
    def cwd(self):
        return Path.cwd()

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            # Backticks in for-arith expressions
            ("for (( i=`rm foo`; i<10; i++ )); do echo $i; done", "ask"),
            # Backticks in param expansion
            ("echo ${x:-`rm foo`}", "ask"),
        ],
    )
    def test_backtick_cmdsub(self, cmd, expected, config, cwd):
        """Backtick command substitutions should be analyzed."""
        assert analyze(cmd, config, cwd).action == expected


class TestHeredocCmdsub:
    """Tests for command substitutions in heredocs."""

    @pytest.fixture
    def config(self):
        return Config()

    @pytest.fixture
    def cwd(self):
        return Path.cwd()

    @pytest.mark.parametrize(
        "cmd,expected",
        [
            # Unquoted heredoc - cmdsubs ARE executed
            ("cat <<EOF\n$(rm foo)\nEOF", "ask"),
            # Multiple cmdsubs in heredoc
            ("cat <<EOF\n$(echo a)\n$(rm foo)\nEOF", "ask"),
        ],
    )
    def test_heredoc_cmdsub(self, cmd, expected, config, cwd):
        """Cmdsubs in unquoted heredocs should be analyzed."""
        assert analyze(cmd, config, cwd).action == expected


class TestCdPathResolution:
    """Test that `cd <literal> && ...` resolves paths against the cd target."""

    def test_cd_resolves_relative_path_for_config_match(self, tmp_path):
        """cd /foo && ./bar should resolve ./bar against /foo."""
        from dippy.core.config import parse_config

        target_dir = tmp_path / "myproject"
        target_dir.mkdir()
        # cd is context-aware, so we need to allow it explicitly
        config = parse_config(f"allow cd *\nallow {target_dir}/tool *")
        # cwd is tmp_path, but cd changes to target_dir
        result = analyze(f"cd {target_dir} && ./tool --flag", config, tmp_path)
        assert result.action == "allow"

    def test_cd_tilde_path(self):
        """cd ~ && ./script should resolve ./script against home."""
        from dippy.core.config import parse_config

        home = Path.home()
        # cd is context-aware, so we need to allow it explicitly
        config = parse_config(f"allow cd *\nallow {home}/script *")
        result = analyze("cd ~ && ./script arg", config, Path("/somewhere/else"))
        assert result.action == "allow"


class TestSubshellContext:
    """Test that @subshell context flag is set correctly in analyzer."""

    def test_cd_in_subshell_allowed(self, tmp_path):
        """cd inside subshell matches [@subshell] rule."""
        from dippy.core.config import parse_config

        config = parse_config("allow [@subshell] cd *")
        result = analyze("(cd /tmp)", config, tmp_path)
        assert result.action == "allow"

    def test_cd_outside_subshell_denied(self, tmp_path):
        """cd outside subshell doesn't match [@subshell] rule."""
        from dippy.core.config import parse_config

        config = parse_config("deny cd *\nallow [@subshell] cd *")
        result = analyze("cd /tmp", config, tmp_path)
        assert result.action == "deny"

    def test_cd_in_compound_not_subshell(self, tmp_path):
        """cd in compound command (&&) is NOT in subshell context."""
        from dippy.core.config import parse_config

        config = parse_config("deny cd *\nallow [@subshell] cd *")
        # cd && ls is NOT a subshell, so cd should be denied
        result = analyze("cd /tmp && ls", config, tmp_path)
        assert result.action == "deny"

    def test_cd_in_subshell_with_compound(self, tmp_path):
        """cd inside subshell with compound is in subshell context."""
        from dippy.core.config import parse_config

        config = parse_config("deny cd *\nallow [@subshell] cd *\nallow ls *")
        # (cd && ls) - entire command is in subshell
        result = analyze("(cd /tmp && ls)", config, tmp_path)
        assert result.action == "allow"

    def test_nested_subshell(self, tmp_path):
        """Nested subshells both have @subshell context."""
        from dippy.core.config import parse_config

        config = parse_config("allow [@subshell] cd *")
        result = analyze("((cd /tmp))", config, tmp_path)
        assert result.action == "allow"

    def test_brace_group_not_subshell(self, tmp_path):
        """Brace group {} is NOT a subshell."""
        from dippy.core.config import parse_config

        config = parse_config("deny cd *\nallow [@subshell] cd *")
        result = analyze("{ cd /tmp; }", config, tmp_path)
        assert result.action == "deny"

    def test_subshell_with_other_commands(self, tmp_path):
        """Commands in subshell inherit @subshell context."""
        from dippy.core.config import parse_config

        config = parse_config("allow [@subshell] rm *")
        result = analyze("(rm /tmp/x)", config, tmp_path)
        assert result.action == "allow"
        # Same command outside subshell shouldn't match
        config2 = parse_config("allow [@subshell] rm *")
        result2 = analyze("rm /tmp/x", config2, tmp_path)
        assert result2.action == "ask"

    def test_cmdsub_inside_subshell(self, tmp_path):
        """Command substitution inside subshell has @subshell context."""
        from dippy.core.config import parse_config

        # echo inside $() inside () should have @subshell from outer ()
        config = parse_config("allow [@subshell] echo *\nallow ls *")
        result = analyze("(ls $(echo test))", config, tmp_path)
        assert result.action == "allow"


class TestBracegroupContext:
    """Test that @bracegroup context flag is set correctly in analyzer."""

    def test_cd_in_bracegroup_allowed(self, tmp_path):
        """cd inside brace group matches [@bracegroup] rule."""
        from dippy.core.config import parse_config

        config = parse_config("allow [@bracegroup] cd *")
        result = analyze("{ cd /tmp; }", config, tmp_path)
        assert result.action == "allow"

    def test_cd_outside_bracegroup_denied(self, tmp_path):
        """cd outside brace group doesn't match [@bracegroup] rule."""
        from dippy.core.config import parse_config

        config = parse_config("deny cd *\nallow [@bracegroup] cd *")
        result = analyze("cd /tmp", config, tmp_path)
        assert result.action == "deny"

    def test_bracegroup_not_subshell(self, tmp_path):
        """Brace group is NOT a subshell - @subshell rule shouldn't match."""
        from dippy.core.config import parse_config

        config = parse_config("deny cd *\nallow [@subshell] cd *")
        result = analyze("{ cd /tmp; }", config, tmp_path)
        assert result.action == "deny"

    def test_subshell_not_bracegroup(self, tmp_path):
        """Subshell is NOT a brace group - @bracegroup rule shouldn't match."""
        from dippy.core.config import parse_config

        config = parse_config("deny cd *\nallow [@bracegroup] cd *")
        result = analyze("(cd /tmp)", config, tmp_path)
        assert result.action == "deny"

    def test_nested_bracegroup(self, tmp_path):
        """Nested brace groups have @bracegroup context."""
        from dippy.core.config import parse_config

        config = parse_config("allow [@bracegroup] cd *")
        result = analyze("{ { cd /tmp; }; }", config, tmp_path)
        assert result.action == "allow"


class TestPipelineContext:
    """Test that @pipeline context flag is set correctly in analyzer."""

    def test_cmd_in_pipeline_allowed(self, tmp_path):
        """Command in pipeline matches [@pipeline] rule."""
        from dippy.core.config import parse_config

        config = parse_config("allow [@pipeline] rm *\nallow cat *")
        result = analyze("cat file | rm -rf /", config, tmp_path)
        assert result.action == "allow"

    def test_cmd_outside_pipeline_denied(self, tmp_path):
        """Command outside pipeline doesn't match [@pipeline] rule."""
        from dippy.core.config import parse_config

        config = parse_config("deny rm *\nallow [@pipeline] rm *")
        result = analyze("rm -rf /", config, tmp_path)
        assert result.action == "deny"

    def test_all_commands_in_pipeline_have_flag(self, tmp_path):
        """All commands in pipeline have @pipeline context."""
        from dippy.core.config import parse_config

        # Both cat and grep should match [@pipeline] rule
        config = parse_config("allow [@pipeline] cat *\nallow [@pipeline] grep *")
        result = analyze("cat file | grep pattern", config, tmp_path)
        assert result.action == "allow"

    def test_list_not_pipeline(self, tmp_path):
        """List (&&) is NOT a pipeline - @pipeline rule shouldn't match."""
        from dippy.core.config import parse_config

        config = parse_config("deny rm *\nallow [@pipeline] rm *\nallow ls *")
        result = analyze("ls && rm file", config, tmp_path)
        assert result.action == "deny"


class TestCompoundContext:
    """Test that @compound context flag covers all compound contexts."""

    def test_compound_in_subshell(self, tmp_path):
        """Subshell has @compound context."""
        from dippy.core.config import parse_config

        config = parse_config("allow [@compound] cd *")
        result = analyze("(cd /tmp)", config, tmp_path)
        assert result.action == "allow"

    def test_compound_in_bracegroup(self, tmp_path):
        """Brace group has @compound context."""
        from dippy.core.config import parse_config

        config = parse_config("allow [@compound] cd *")
        result = analyze("{ cd /tmp; }", config, tmp_path)
        assert result.action == "allow"

    def test_compound_in_pipeline(self, tmp_path):
        """Pipeline has @compound context."""
        from dippy.core.config import parse_config

        config = parse_config("allow [@compound] cat *\nallow [@compound] grep *")
        result = analyze("cat file | grep pattern", config, tmp_path)
        assert result.action == "allow"

    def test_compound_in_list(self, tmp_path):
        """List (&&, ||) has @compound context."""
        from dippy.core.config import parse_config

        config = parse_config("allow [@compound] ls *\nallow [@compound] echo *")
        result = analyze("ls && echo done", config, tmp_path)
        assert result.action == "allow"

    def test_compound_not_in_simple_command(self, tmp_path):
        """Simple command does NOT have @compound context."""
        from dippy.core.config import parse_config

        config = parse_config("deny cd *\nallow [@compound] cd *")
        result = analyze("cd /tmp", config, tmp_path)
        assert result.action == "deny"

    def test_compound_covers_all_contexts(self, tmp_path):
        """@compound matches subshell, bracegroup, pipeline, and list."""
        from dippy.core.config import parse_config

        # Single rule should match all compound contexts
        config = parse_config("allow [@compound] rm *")

        # Subshell
        result = analyze("(rm file)", config, tmp_path)
        assert result.action == "allow", "subshell should have @compound"

        # Brace group
        result = analyze("{ rm file; }", config, tmp_path)
        assert result.action == "allow", "bracegroup should have @compound"

        # Pipeline
        result = analyze("cat x | rm file", config, tmp_path)
        assert result.action == "allow", "pipeline should have @compound"

        # List
        result = analyze("ls && rm file", config, tmp_path)
        assert result.action == "allow", "list should have @compound"

    def test_specific_flag_with_compound(self, tmp_path):
        """Specific flags work alongside @compound."""
        from dippy.core.config import parse_config

        # Rule requires both @subshell AND @compound
        config = parse_config("allow [@subshell,@compound] cd *")
        result = analyze("(cd /tmp)", config, tmp_path)
        assert result.action == "allow"

        # Brace group has @compound but not @subshell
        config2 = parse_config("deny cd *\nallow [@subshell,@compound] cd *")
        result2 = analyze("{ cd /tmp; }", config2, tmp_path)
        assert result2.action == "deny"


class TestReasonFormatNoRedundantBase:
    """Test that reason string doesn't duplicate command name.

    When a config pattern includes the command name (e.g., "mkdir -p ./**"),
    the reason should NOT be "mkdir (mkdir -p ./**)".
    It should be just the pattern or formatted without redundancy.
    """

    def test_reason_no_redundant_base_mkdir(self, tmp_path):
        """mkdir -p ./foo should not produce 'mkdir (mkdir -p ...)'."""
        from dippy.core.config import parse_config

        config = parse_config("allow mkdir -p ./**")
        result = analyze("mkdir -p ./foo/bar", config, tmp_path)
        assert result.action == "allow"
        # The reason should be just the pattern, no redundant prefix or parentheses
        assert result.reason == "mkdir -p ./**"

    def test_reason_no_redundant_base_mv(self, tmp_path):
        """mv with pattern should not duplicate 'mv' in reason."""
        from dippy.core.config import parse_config

        config = parse_config(
            "allow mv ./.claude/diary/*.md ./.claude/diary/processed/"
        )
        result = analyze(
            "mv ./.claude/diary/test.md ./.claude/diary/processed/", config, tmp_path
        )
        assert result.action == "allow"
        # The reason should be just the pattern
        assert result.reason == "mv ./.claude/diary/*.md ./.claude/diary/processed/"

    def test_reason_simple_pattern_still_works(self, tmp_path):
        """Simple patterns like 'git status' should show just the pattern."""
        from dippy.core.config import parse_config

        config = parse_config("allow git status")
        result = analyze("git status", config, tmp_path)
        assert result.action == "allow"
        # Pattern starts with base, so reason is just the pattern
        assert result.reason == "git status"

    def test_reason_with_wildcards(self, tmp_path):
        """Patterns with wildcards should show just the pattern."""
        from dippy.core.config import parse_config

        config = parse_config("allow ls -la ./**")
        result = analyze("ls -la ./src/foo.py", config, tmp_path)
        assert result.action == "allow"
        # Pattern starts with base, so reason is just the pattern
        assert result.reason == "ls -la ./**"

    def test_reason_pattern_with_star_suffix(self, tmp_path):
        """Patterns like 'echo *' should show just the pattern."""
        from dippy.core.config import parse_config

        # Pattern is "echo *" which starts with base "echo"
        config = parse_config("allow echo *")
        result = analyze("echo hello", config, tmp_path)
        assert result.action == "allow"
        # Pattern starts with base, so reason is just the pattern
        assert result.reason == "echo *"

    def test_reason_just_test_no_parentheses(self, tmp_path):
        """'just test *' pattern should show 'just test *', not '(just test *)'."""
        from dippy.core.config import parse_config

        # This is the exact case from user's audit log
        config = parse_config("allow just test *")
        result = analyze("just test foo", config, tmp_path)
        assert result.action == "allow"
        # Pattern starts with base, so reason is just the pattern without parentheses
        assert result.reason == "just test *"
        assert not result.reason.startswith("(")

    def test_reason_pipeline_no_parentheses(self, tmp_path):
        """Pipeline 'just test foo | head' should show 'just test *, head' not '(just test *), head'."""
        from dippy.core.config import parse_config

        # This is the exact case from user's audit log:
        # {"cmd": "(just test *), head", "command": "just test ... 2>&1 | head -30"}
        config = parse_config("allow just test *\nallow head *")
        result = analyze("just test foo 2>&1 | head -30", config, tmp_path)
        assert result.action == "allow"
        # The reason should NOT have parentheses around 'just test *'
        assert "(just" not in result.reason
        # Should be "just test *, head *" or similar clean format
        assert "just test *" in result.reason

    def test_reason_pattern_without_base_keeps_format(self, tmp_path):
        """Patterns that don't start with command should show 'base (pattern)'."""
        from dippy.core.config import parse_config

        # Pattern is "--help" which doesn't start with "git"
        config = parse_config("allow git --help")
        result = analyze("git --help", config, tmp_path)
        assert result.action == "allow"
        # Pattern "git --help" starts with "git", so just pattern
        assert result.reason == "git --help"
