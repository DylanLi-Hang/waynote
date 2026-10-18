#!/usr/bin/env python3
"""Build and exercise Waynote without installing or activating a NixOS generation."""

import argparse
import fcntl
import json
import os
from pathlib import Path
import re
import shlex
import signal
import subprocess
import sys
import time
import tomllib

REPO = Path(__file__).resolve().parent.parent
WORK = REPO / "target" / "dev"
STATE = WORK / "session.json"
DESCRIPTION = "Waynote development (temporary)"
APP_CLASS = "dev.mryll.waynote.development"


def execute(*args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def dev_env(root=WORK):
    return dict(os.environ, WAYNOTE_DEV="1", GDK_BACKEND="wayland",
                XDG_DATA_HOME=str(root / "data"),
                XDG_STATE_HOME=str(root / "state"),
                XDG_CONFIG_HOME=str(root / "config"))


def seed(root=WORK):
    notes = root / "data" / "waynote" / "notes"
    notes.mkdir(parents=True, exist_ok=True)
    if not any(notes.glob("*.md")):
        (notes / "01JZ9P6S0R8ZX0G8N3Z4V7Y8QK-dev.md").write_text(
            '---\nid: 01JZ9P6S0R8ZX0G8N3Z4V7Y8QK\ncolor: blue\n---\n'
            '# [DEV] Waynote\n\nIsolated development notes.\n\n'
            '- [ ] Try a checkbox\n- [x] Development data is separate\n\n'
            'Double-click text to edit; click the title or hide to show Preview.\n')


def binds():
    result = execute("hyprctl", "binds", "-j", capture_output=True, text=True)
    return [b for b in json.loads(result.stdout)
            if b.get("modmask") == 64 and b.get("key", "").lower() == "n"
            and not b.get("submap")]


def lua(code):
    result = execute("hyprctl", "eval", code, capture_output=True, text=True)
    if result.stdout.strip() != "ok":
        raise RuntimeError(f"Hyprland Lua failed: {result.stdout} {result.stderr}")


def dev_client(state):
    result = execute("hyprctl", "clients", "-j", capture_output=True, text=True)
    return next((window for window in json.loads(result.stdout)
                 if window.get("class") == APP_CLASS
                 and window.get("pid") == state.get("child_pid")), None)


def float_window(state):
    """Change only this session's mapped dev window; no persistent rules to undo."""
    if not os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"):
        return
    deadline = time.monotonic() + 3
    while not dev_client(state):
        if time.monotonic() >= deadline:
            raise RuntimeError("Development board did not map within 3 seconds")
        time.sleep(0.1)
    # Mapping in a tiling layout can overwrite GTK's requested note size.
    try:
        geometry = tomllib.loads((WORK / "state/waynote/layout.toml").read_text())
        width, height = int(geometry["width"]), int(geometry["height"])
    except (OSError, ValueError, KeyError, TypeError):
        width, height = 400, 440
    lua('for _, window in ipairs(hl.get_windows()) do '
        f'if window.class == {json.dumps(APP_CLASS)} and window.pid == {int(state["child_pid"])} then '
        'hl.dispatch(hl.dsp.window.float({ window = window, action = "enable" })); '
        f'hl.dispatch(hl.dsp.window.resize({{ window = window, x = {max(360, width)}, '
        f'y = {max(300, height)}, relative = false }})) end end')


def original_binding(current, config):
    """Lua callbacks cannot be reconstructed from hyprctl's numeric callback ID."""
    if len(current) != 1:
        raise RuntimeError("--bind needs exactly one existing Meta+N binding")
    binding = current[0]
    if binding["dispatcher"] == "exec":
        return (f'hl.bind("SUPER + N", hl.dsp.exec_cmd({json.dumps(binding["arg"])}), '
                f'{{ description = {json.dumps(binding.get("description", ""))} }})')
    # This machine uses Home Manager's generated, single-line Lua bindings.
    # Match the active description: configs may include a conditional fallback
    # binding for another shell. Duplicate active candidates remain ambiguous.
    lines = [line.strip() for line in config.read_text().splitlines()
             if re.match(r'^\s*hl\.bind\("SUPER \+ N"\s*,', line)
             and json.dumps(binding.get("description", "")) in line]
    if (binding["dispatcher"] != "__lua" or len(lines) != 1
            or 'hl.dsp.exec_cmd(' not in lines[0]
            or json.dumps(binding.get("description", "")) not in lines[0]):
        raise RuntimeError("Cannot safely recover Meta+N from this Lua config; run without --bind")
    return lines[0]


def restore_binding(state):
    if not state.get("restore"):
        return
    if state.get("hyprland") != os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"):
        raise RuntimeError("Restore Meta+N in the Hyprland session that started this test")
    current = binds()
    # A compositor reload or user change wins over our saved binding.
    if not current or (len(current) == 1 and current[0].get("description") == DESCRIPTION):
        lua('hl.unbind("SUPER + N"); ' + state["restore"])
        print("Restored the original Meta+N binding.", flush=True)


def write_state(state):
    temporary = STATE.with_suffix(".tmp")
    temporary.write_text(json.dumps(state, indent=2))
    temporary.replace(STATE)


def owned_child(state):
    pid = state.get("child_pid")
    if not pid:
        return False
    try:
        cmdline = Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0")
        if cmdline[0] != os.fsencode(REPO / "target/debug/waynote"):
            return False
        environment = Path(f"/proc/{pid}/environ").read_bytes().split(b"\0")
        return (b"WAYNOTE_DEV=1" in environment
                and os.fsencode("XDG_DATA_HOME=" + str(WORK / "data")) in environment)
    except (FileNotFoundError, ProcessLookupError):
        return False
    except PermissionError:
        # The child can exit between the cmdline and environ reads. Only a
        # proven dead process is safe to forget; keep live unreadable state.
        try:
            status = Path(f"/proc/{pid}/status").read_text()
        except (FileNotFoundError, ProcessLookupError):
            return False
        if any(line.startswith("State:") and line.split()[1] in {"Z", "X"}
               for line in status.splitlines()):
            return False
        raise


def quit_child(state):
    # A stale PID must never start a fresh app merely to send it `quit`.
    if not owned_child(state):
        return
    execute(str(REPO / "target/debug/waynote"), "quit", env=dev_env(), timeout=5)
    # The CLI can succeed even when a save error makes the board reject quit.
    deadline = time.monotonic() + 5
    while owned_child(state):
        if time.monotonic() >= deadline:
            raise RuntimeError("Development board kept running. Resolve its save error and retry scripts/dev stop.")
        time.sleep(0.1)


def read_state():
    return json.loads(STATE.read_text()) if STATE.exists() else {}


def owned_process(state):
    try:
        cmdline = Path(f'/proc/{state["pid"]}/cmdline').read_bytes().split(b"\0")
        return os.fsencode(Path(__file__).resolve()) in cmdline
    except (KeyError, FileNotFoundError, ProcessLookupError):
        return False


def stop():
    state = read_state()
    if owned_process(state):
        os.kill(state["pid"], signal.SIGTERM)
        deadline = time.monotonic() + 10
        while STATE.exists() and time.monotonic() < deadline:
            time.sleep(0.1)
        if STATE.exists():
            raise RuntimeError("Development session did not stop; inspect target/dev/app.log")
    elif state:
        try:
            quit_child(state)
        finally:
            restore_binding(state)
            if not owned_child(state):
                STATE.unlink(missing_ok=True)


def run(take_binding):
    execute("cargo", "build", "--locked", cwd=REPO)
    stop()
    WORK.mkdir(parents=True, exist_ok=True)
    with (WORK / "session.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        seed()
        state = {"pid": os.getpid(), "hyprland": os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")}
        if take_binding:
            config = Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config"))) / "hypr" / "hyprland.lua"
            state["restore"] = original_binding(binds(), config)
        child = None
        # Save recovery information before mutating a compositor binding.
        write_state(state)
        def interrupted(_signum, _frame):
            raise KeyboardInterrupt
        for sig in (signal.SIGTERM, signal.SIGINT):
            signal.signal(sig, interrupted)
        try:
            with (WORK / "app.log").open("a") as log:
                child = subprocess.Popen([str(REPO / "target" / "debug" / "waynote")],
                                         cwd=REPO, env=dev_env(), stdout=log, stderr=log,
                                         start_new_session=True)
                state["child_pid"] = child.pid
                write_state(state)
                float_window(state)
                if take_binding:
                    command = shlex.join([str(REPO / "scripts" / "dev"), "toggle"])
                    lua('hl.unbind("SUPER + N"); '
                        f'hl.bind("SUPER + N", hl.dsp.exec_cmd({json.dumps(command)}), '
                        f'{{ description = {json.dumps(DESCRIPTION)} }})')
                print(f"Waynote DEV — data and logs: {WORK}\n"
                      "Ctrl+C or scripts/dev stop ends this session.", flush=True)
                if child.wait() != 0:
                    raise RuntimeError("Development app failed; inspect target/dev/app.log")
        except KeyboardInterrupt:
            pass
        finally:
            try:
                if child is not None and child.poll() is None:
                    quit_child(state)
            finally:
                restore_binding(state)
                if not owned_child(state):
                    STATE.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", nargs="?", default="run",
                        choices=["run", "stop", "status", "check", "gui", "package",
                                 "new", "toggle", "show-all", "hide-all"])
    parser.add_argument("--bind", action="store_true", help="temporarily use Meta+N (Hyprland Lua)")
    args = parser.parse_args()
    if args.command == "run":
        run(args.bind)
    elif args.command == "stop":
        stop()
    elif args.command == "status":
        state = read_state()
        print("Running" if owned_process(state) or owned_child(state) else "Stopped")
        print(f"Data and logs: {WORK}")
    elif args.command == "check":
        execute(sys.executable, "-m", "unittest", "discover", "-s", "scripts", "-p", "test_dev.py", cwd=REPO)
        execute("nix", "eval", f".#packages.{os.uname().machine}-linux.default.drvPath", cwd=REPO)
        execute("cargo", "test", "--locked", cwd=REPO)
        execute("cargo", "clippy", "--locked", "--all-targets", cwd=REPO)
    elif args.command == "gui":
        execute(sys.executable, "scripts/gui-regressions.py", cwd=REPO)
    elif args.command == "package":
        execute("nix", "build", ".#default", "--no-link", "--print-out-paths", cwd=REPO)
    else:
        state = read_state()
        if not owned_child(state):
            raise RuntimeError("Start scripts/dev run before sending development commands")
        hyprland = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
        showing = args.command in ("new", "show-all") or (
            args.command == "toggle" and hyprland and not dev_client(state))
        execute(str(REPO / "target" / "debug" / "waynote"), args.command, env=dev_env())
        if showing:
            float_window(state)


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
