"""Run real-pointer GUI regressions without touching the desktop seat or notes."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time


def main():
    # No service directories: private tests must not activate desktop portals.
    if "--private-bus" not in sys.argv:
        with tempfile.TemporaryDirectory(prefix="waynote-test-bus-") as directory:
            config = Path(directory) / "bus.conf"
            config.write_text('''<busconfig>
  <type>session</type><listen>unix:tmpdir=/tmp</listen>
  <policy context="default">
    <allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/>
  </policy>
</busconfig>''')
            return subprocess.call([
                "dbus-run-session", f"--config-file={config}", "--",
                sys.executable, __file__, "--private-bus",
            ])

    sway = os.environ.get("WAYNOTE_TEST_SWAY") or shutil.which("sway")
    if not sway:
        raise SystemExit("Install Sway or set WAYNOTE_TEST_SWAY to its executable")
    repo = Path(__file__).resolve().parent.parent
    probe = repo / "target/test-pointer"
    subprocess.run([
        "cargo", "build", "--locked", "--manifest-path",
        str(repo / "scripts/test-pointer/Cargo.toml"), "--target-dir", str(probe),
    ], check=True)
    artifacts = Path(tempfile.mkdtemp(prefix="waynote-live-drag-", dir="/tmp"))
    print("artifacts", artifacts, flush=True)
    with tempfile.TemporaryDirectory(prefix="waynote-focus-runtime-", dir="/tmp") as directory:
        runtime = Path(directory)
        config = runtime / "sway.conf"
        config.write_text(
            'output HEADLESS-1 mode 1280x900 position 0 0\n'
            'seat seat0 fallback true\nseat seat0 attach "*"\n'
        )
        env = {
            **os.environ,
            "XDG_RUNTIME_DIR": str(runtime),
            "WLR_HEADLESS_OUTPUTS": "1", "WLR_BACKENDS": "headless",
            "WLR_RENDERER": "pixman", "GDK_BACKEND": "wayland",
            "GSK_RENDERER": "cairo", "GTK_USE_PORTAL": "0", "GIO_USE_VFS": "local",
            "XDG_CURRENT_DESKTOP": "sway", "WAYNOTE_LAYER_SHELL": "1",
            "WAYNOTE_DEV": "1", "WAYNOTE_GEOMETRY_LOG": str(artifacts / "geometry"),
            "WAYNOTE_TEST_POINTER": str(probe / "debug/wayland-input-prototype"),
        }
        for key in ["WAYLAND_DISPLAY", "DISPLAY", "SWAYSOCK", "HYPRLAND_INSTANCE_SIGNATURE"]:
            env.pop(key, None)
        for kind in ["DATA", "STATE", "CONFIG"]:
            env[f"XDG_{kind}_HOME"] = str(artifacts / kind.lower())
        with (artifacts / "sway.log").open("w") as log:
            compositor = subprocess.Popen(
                [sway, "-c", str(config)], env=env, stdout=log, stderr=subprocess.STDOUT,
            )
            try:
                deadline = time.monotonic() + 10
                while True:
                    socket = next((p for p in runtime.glob("wayland-*") if p.is_socket()), None)
                    if socket:
                        env["WAYLAND_DISPLAY"] = socket.name
                        break
                    if compositor.poll() is not None or time.monotonic() > deadline:
                        raise RuntimeError(f"Private compositor failed; see {artifacts / 'sway.log'}")
                    time.sleep(0.05)
                subprocess.run([
                    "cargo", "test", "--locked", "--offline", "--features",
                    "layer-shell-prototype", "layer_gui_regressions", "--",
                    "--ignored", "--test-threads=1", "--nocapture",
                ], cwd=repo, env=env, check=True, timeout=90)
                # Keep the ordinary-window controller behavior covered as well.
                normal_env = {**env, "WAYNOTE_LAYER_SHELL": "0"}
                subprocess.run([
                    "cargo", "test", "--locked", "--offline", "--features",
                    "layer-shell-prototype", "board_gui_regressions", "--",
                    "--ignored", "--test-threads=1", "--nocapture",
                ], cwd=repo, env=normal_env, check=True, timeout=90)
            finally:
                if compositor.poll() is None:
                    compositor.terminate()
                    try:
                        compositor.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        compositor.kill()
                        compositor.wait()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
