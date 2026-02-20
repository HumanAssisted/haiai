"""Tests for haisdk CLI passthrough to the JACS CLI."""

from __future__ import annotations

from types import SimpleNamespace

import pytest

from jacs.hai import cli as hai_cli


def test_explicit_jacs_passthrough_invokes_jacs_binary(monkeypatch: pytest.MonkeyPatch) -> None:
    seen: dict[str, list[str]] = {}

    def fake_run(command: list[str], check: bool = False):  # noqa: ARG001
        seen["command"] = command
        return SimpleNamespace(returncode=0)

    monkeypatch.setattr(hai_cli.subprocess, "run", fake_run)

    with pytest.raises(SystemExit) as exit_info:
        hai_cli.main(["jacs", "agent", "lookup", "example.com"])

    assert exit_info.value.code == 0
    assert seen["command"] == ["jacs", "agent", "lookup", "example.com"]


def test_unknown_command_passthrough_invokes_jacs_binary(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    seen: dict[str, list[str]] = {}

    def fake_run(command: list[str], check: bool = False):  # noqa: ARG001
        seen["command"] = command
        return SimpleNamespace(returncode=0)

    monkeypatch.setattr(hai_cli.subprocess, "run", fake_run)

    with pytest.raises(SystemExit) as exit_info:
        hai_cli.main(["document", "verify", "-f", "signed.json"])

    assert exit_info.value.code == 0
    assert seen["command"] == ["jacs", "document", "verify", "-f", "signed.json"]


def test_passthrough_honors_custom_jacs_cli_binary(monkeypatch: pytest.MonkeyPatch) -> None:
    seen: dict[str, list[str]] = {}

    def fake_run(command: list[str], check: bool = False):  # noqa: ARG001
        seen["command"] = command
        return SimpleNamespace(returncode=0)

    monkeypatch.setenv("JACS_CLI_BIN", "/custom/jacs")
    monkeypatch.setattr(hai_cli.subprocess, "run", fake_run)

    with pytest.raises(SystemExit) as exit_info:
        hai_cli.main(["agent", "verify"])

    assert exit_info.value.code == 0
    assert seen["command"] == ["/custom/jacs", "agent", "verify"]


def test_verify_command_passthrough_invokes_jacs_binary(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    seen: dict[str, list[str]] = {}

    def fake_run(command: list[str], check: bool = False):  # noqa: ARG001
        seen["command"] = command
        return SimpleNamespace(returncode=0)

    monkeypatch.setattr(hai_cli.subprocess, "run", fake_run)

    with pytest.raises(SystemExit) as exit_info:
        hai_cli.main(["verify", "signed.json"])

    assert exit_info.value.code == 0
    assert seen["command"] == ["jacs", "verify", "signed.json"]


def test_help_is_merged_with_jacs_help(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    def fake_run(
        command: list[str],
        check: bool = False,  # noqa: ARG001
        capture_output: bool = False,  # noqa: ARG001
        text: bool = False,  # noqa: ARG001
    ):
        assert command == ["jacs", "--help"]
        return SimpleNamespace(returncode=0, stdout="Usage: jacs [COMMAND]\n", stderr="")

    monkeypatch.setattr(hai_cli.subprocess, "run", fake_run)

    with pytest.raises(SystemExit) as exit_info:
        hai_cli.main(["--help"])

    out = capsys.readouterr().out
    assert exit_info.value.code == 0
    assert "HAI SDK CLI" in out
    assert "JACS CLI passthrough" in out
    assert "Usage: jacs [COMMAND]" in out
