"""Exercise the release-transfer example on private headless Wayland outputs."""
import os
from pathlib import Path
import select
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent


def wait(predicate, description):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.03)
    raise AssertionError(f"Timed out: {description}")


def scenario(name, output_config, extent, vertical=False):
    artifacts = Path(tempfile.mkdtemp(prefix=f"waynote-{name}-", dir="/tmp"))
    print(f"{name}: {artifacts}", flush=True)
    with tempfile.TemporaryDirectory(prefix="waynote-focus-runtime-", dir="/tmp") as directory:
        runtime = Path(directory)
        config = runtime / "sway.conf"
        config.write_text(output_config + '\nseat seat0 fallback true\nseat seat0 attach "*"\n')
        env = {
            **os.environ, "XDG_RUNTIME_DIR": str(runtime),
            "WLR_BACKENDS": "headless", "WLR_HEADLESS_OUTPUTS": "2",
            "WLR_RENDERER": "pixman", "GSK_RENDERER": "cairo", "GDK_BACKEND": "wayland",
            "GTK_USE_PORTAL": "0", "GIO_USE_VFS": "local", "XDG_CURRENT_DESKTOP": "sway",
            "WAYNOTE_LAYER_SHELL": "1", "WAYNOTE_GEOMETRY_LOG": str(artifacts / "geometry"),
            "WAYNOTE_TEST_WIDTH": str(extent[0]), "WAYNOTE_TEST_HEIGHT": str(extent[1]),
            # Abort on GTK criticals, including invalid widgets during release dispatch.
            "G_DEBUG": "fatal-criticals",
        }
        for key in ["WAYLAND_DISPLAY", "DISPLAY", "SWAYSOCK", "HYPRLAND_INSTANCE_SIGNATURE", "DEMO_TEST_BARS"]:
            env.pop(key, None)
        if vertical:
            env["DEMO_TEST_BARS"] = "1"
        for kind in ["DATA", "STATE", "CONFIG"]:
            env[f"XDG_{kind}_HOME"] = str(artifacts / kind.lower())
        processes, logs = [], []

        def launch(command, label):
            log = (artifacts / f"{label}.log").open("w")
            logs.append(log)
            process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT)
            processes.append(process)
            return process

        def log_text():
            return (artifacts / "demo.log").read_text()

        def rectangle():
            path = artifacts / "geometry"
            lines = path.read_text().splitlines() if path.exists() else []
            return tuple(map(float, lines[-1].split())) if lines else None

        def check(x, y):
            def matches():
                actual = rectangle()
                return actual and abs(actual[0] - x) <= 1 and abs(actual[1] - y) <= 1
            try:
                wait(matches, f"rendered position ({x}, {y})")
            except AssertionError as error:
                raise AssertionError(f"{error}; actual={rectangle()}, logs={artifacts}") from error

        try:
            launch([os.environ.get("WAYNOTE_TEST_SWAY") or shutil.which("sway") or "sway", "-c", str(config)], "sway")
            wait(lambda: any(p.is_socket() for p in runtime.glob("wayland-*")), "Wayland socket")
            env["WAYLAND_DISPLAY"] = next(p.name for p in runtime.glob("wayland-*") if p.is_socket())
            app = launch([str(ROOT / "target/debug/examples/release-transfer")], "demo")
            wait(lambda: "READY" in log_text(), "demo ready")
            check(100, 100)
            # Allow input-region commit after the first layout frame.
            time.sleep(0.15)
            probe = subprocess.Popen(
                [str(ROOT / "target/test-pointer/debug/wayland-input-prototype")],
                env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
            )
            processes.append(probe)

            def send(command):
                assert app.poll() is None, f"demo exited; see {artifacts}"
                probe.stdin.write(command + "\n")
                probe.stdin.flush()
                assert select.select([probe.stdout], [], [], 3)[0], "pointer response timed out"
                assert probe.stdout.readline().strip() == "ok"
                time.sleep(0.12)

            def move(x, y):
                send(f"move {x} {y}")

            transfers = 0

            def release_to(x, y):
                nonlocal transfers
                send("release")
                transfers += 1
                check(x, y)
                time.sleep(0.15)

            # These assertions happen while pressed: drag_end-only code must fail.
            move(140, 140)
            send("press")
            for x, y in [(240, 180), (340, 220), (200, 160), (1, 1), (140, 140)]:
                move(x, y)
                check(max(0, x - 40), max(0, y - 40))
            if vertical:
                # Actual desktop layout: big screen above smaller, offset screen.
                # A 40px exclusive bar on each exposes monitor/work-area confusion.
                for x, y in [(1000, 300), (1200, 1300), (1500, 1800)]:
                    move(x, y)
                release_to(700, 320)
                move(1500, 1800)
                send("press")
                for x, y in [(1600, 1850), (2000, 2000), (1400, 1700)]:
                    move(x, y)
                    check(x - 800, y - 1480)
                move(3200, 600)
                release_to(3080, 560)
                # Starting near the large screen's right edge used to prevent
                # GtkFixed from shrinking on return to the smaller output.
                move(3120, 600)
                send("press")
                move(1500, 1800)
                release_to(700, 320)
                move(1500, 1800)
                send("press")
                move(1700, 1900)
                check(900, 420)
                send("release")
                move(2000, 2100)
                check(900, 420)
            else:
                width = extent[0] - 1280
                far_x = extent[0] - 300
                for x, y in [(700, 200), (1250, 240), (far_x, 320)]:
                    move(x, y)
                    check(min(920, x - 40), y - 40)
                # Far-right destination must use the NEW width, not old 1280.
                release_to(width - 360, 280)
                grab_x = 1280 + width - 360 + 40
                move(grab_x, 320)
                send("press")
                move(grab_x - 80, 360)
                check(width - 440, 320)
                move(700, 400)
                release_to(660, 360)
                move(700, 400)
                send("press")
                move(far_x, 320)
                release_to(width - 360, 280)
                move(1000, 700)
                check(width - 360, 280)
            assert app.poll() is None, f"GTK critical/crash; see {artifacts}"
            assert "assertion" not in log_text() and "CRITICAL" not in log_text(), log_text()
            # Exercise the full production controller, not just the thin example.
            probe.terminate(); probe.wait(timeout=3)
            app.terminate(); app.wait(timeout=3)
            if vertical:
                env["DEMO_BARS_ONLY"] = "1"
                launch([str(ROOT / "target/debug/examples/release-transfer")], "bars")
                time.sleep(0.2)
            env["WAYNOTE_TEST_POINTER"] = str(ROOT / "target/test-pointer/debug/wayland-input-prototype")
            subprocess.run(["cargo", "test", "--locked", "--offline", "--features",
                            "layer-shell-prototype", "layer_transfer_gui_regressions", "--",
                            "--ignored", "--test-threads=1", "--nocapture"],
                           cwd=ROOT, env=env, check=True, timeout=90)
            print(f"PASS {name}: example + production controller", flush=True)
        finally:
            for process in reversed(processes):
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
            for log in logs:
                log.close()


def main():
    if "--private-bus" not in sys.argv:
        with tempfile.TemporaryDirectory(prefix="waynote-test-bus-") as directory:
            config = Path(directory) / "bus.conf"
            # No service directories: never activate the real desktop portals.
            config.write_text('<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen>'
                              '<policy context="default"><allow send_destination="*"/>'
                              '<allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
            return subprocess.call(["dbus-run-session", f"--config-file={config}", "--",
                                    sys.executable, __file__, "--private-bus"])
    subprocess.run(["cargo", "build", "--locked", "--features", "layer-shell-prototype",
                    "--example", "release-transfer", "--target-dir", str(ROOT / "target")], cwd=ROOT, check=True)
    subprocess.run(["cargo", "build", "--locked", "--manifest-path", str(ROOT / "scripts/test-pointer/Cargo.toml"),
                    "--target-dir", str(ROOT / "target/test-pointer")], check=True)
    for name, mode, width in [("equal", "1280x900", 2560),
                              ("unequal", "1920x900", 3200),
                              ("mixed-scale", "1920x1350 scale 1.5", 2560)]:
        scenario(name, f"output HEADLESS-1 mode 1280x900 position 0 0\noutput HEADLESS-2 mode {mode} position 1280 0", (width, 900))
    scenario("vertical-bars", "output HEADLESS-1 mode 3440x1440 position 0 0\n"
             "output HEADLESS-2 mode 1920x1200 position 760 1440", (3440, 2640), vertical=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
