#!/usr/bin/env python3
"""Run the board's GTK regressions in serial on the current Wayland session."""

import os
from pathlib import Path
import signal
import subprocess
import sys


def run_gui_test():
    repo = Path(__file__).resolve().parent.parent
    cargo = ["cargo", "test", "--locked", "--offline", "gui_regressions"]
    # Keep cold compilation outside the execution timeout. The Rust test owns
    # its temporary notes/state so developer and installed data stay untouched.
    subprocess.run(cargo + ["--no-run"], cwd=repo, check=True)
    proc = subprocess.Popen(
        cargo + ["--", "--ignored", "--test-threads=1", "--nocapture"],
        cwd=repo,
        env=dict(os.environ, GDK_BACKEND="wayland", WAYNOTE_DEV="1"),
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )
    try:
        output, _ = proc.communicate(timeout=120)
    except subprocess.TimeoutExpired:
        # Cargo's test child must not outlive the timed-out runner.
        os.killpg(proc.pid, signal.SIGKILL)
        output, _ = proc.communicate()
        print(output, file=sys.stderr)
        raise SystemExit("GTK regression test timed out after 120 seconds")
    return proc.returncode, output


def main():
    returncode, output = run_gui_test()
    if returncode:
        print(output, file=sys.stderr)
        raise SystemExit(f"GTK regression test exited with {returncode}")
    print(output, end="")


if __name__ == "__main__":
    main()
