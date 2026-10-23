# Developing on NixOS

Build and run without installing or switching the system:

```sh
./scripts/dev run
```

The runner enters the pinned Nix development shell, builds incrementally, and
starts the app in the foreground. Run it again to rebuild and replace the prior
development session. Ctrl+C or `./scripts/dev stop` stops it.

The app uses D-Bus/application ID `dev.mryll.waynote.development`. Notes, window
state, configuration and logs live under `target/dev/`, separate from the
installed app. The first run creates a blue `[DEV] Waynote` sample; subsequent
runs preserve development data. Editing autosaves, and hiding flushes drafts.
A save error keeps the draft visible so it can be copied or retried.

## Meta+N on Hyprland

```sh
./scripts/dev run --bind
```

While the runner lives, Meta+N drives the development app. The original binding
is restored on normal exit or Ctrl+C/SIGTERM. Temporary, non-consuming mouse
bindings also save and return to Preview when another window is clicked; moving
the pointer away keeps Edit active. These bindings are removed by their own
handles on stop, without touching other mouse bindings. The helper also keeps
the note’s floating position across hide/show on the same monitor and removes
its map/unmap event subscriptions on stop. Only the running
compositor is changed; no NixOS or Home Manager activation is performed. The runner floats
only its own development window and restores the saved note size (400 × 440
on the first run), instead of keeping the compositor's initial tiling size.

The binding helper uses Hyprland's Lua API. It recovers the original single-line
`hl.bind("SUPER + N", hl.dsp.exec_cmd(...), ...)` from
`$XDG_CONFIG_HOME/hypr/hyprland.lua` and refuses ambiguous bindings. Run without
`--bind` on unsupported desktops or configurations.

After SIGKILL, run `./scripts/dev stop` in the same Hyprland session to recover
the saved binding. A compositor reload or a user-installed binding takes
precedence over restoration.

```sh
./scripts/dev toggle
./scripts/dev new
./scripts/dev status
./scripts/dev stop
```

These commands address only the development instance. The installed `waynote`
command continues to address the installed instance.

## Checks

```sh
./scripts/dev check       # Python runner tests, Nix evaluation, Rust tests, Clippy
./scripts/dev gui         # isolated GTK behavior regressions in the Wayland session
./scripts/dev package     # wrapped Nix package build, no installation or activation
```

The first build fetches pinned dependencies and compiles Cargo crates. Later
builds reuse the checkout's `target/` artifacts. GUI tests require an active
Wayland session and briefly open a test window. Tests cover state/persistence
logic and GTK behavior. On Hyprland, `gui` also runs isolated Lua policy tests
for outside clicks, position restoration and handle cleanup; use the real
compositor to evaluate focus, shortcuts, stacking and resizing. Hyprland results do not establish KDE compatibility.

Logs are appended to `target/dev/app.log`; saved development notes are under
`target/dev/data/waynote/notes/`. The note uses a regular GTK window, so its
floating and stacking policy belongs to the compositor; see the README.

## Layer Shell development

The `develop` branch provides `./scripts/layer-dev` for the
real note interface using Layer Shell and isolated notes, plus a protocol probe.
See [the migration guide](layer-shell-prototype.md) for launch commands, test
scope and migration gates. The normal development runner above
still exercises the existing regular-window application.

## Develop acceptance before stable promotion

`main` is stable. Runtime changes and their regression tests land on `develop`
first. The cross-screen release was rolled back because headless tests did not
cover existing large-screen geometry on a smaller screen and the actual resident
process was still an older executable after the OS update.

Before proposing promotion:

1. Run unit tests, both controller GUI suites, Clippy, and the four-layout
   `scripts/test-release-transfer.py` suite on develop. Include existing saved
   positions/sizes, not just default fresh state.
2. Start the production UI with isolated development notes/state on the real
   desktop. Confirm its PID/executable is the build being tested. Rebuilding or
   updating the system does not replace a resident process; another invocation
   can merely send an action to that old process.
3. Verify smaller/larger screens, reserved bars, title dragging, edit/Preview,
   menus, resize, cross-screen round trips and restored state. Check that the
   rendered panel and clickable region agree, including after startup.
4. Wait for the user's explicit acceptance of develop and authorization to
   promote. Only then merge develop into main and update the stable consumer pin.

The isolated Layer Shell app uses `dev.mryll.waynote.layershell.development`.
Do not terminate the installed instance or modify its notes just to test a build.
A normal Quit must be allowed to flush drafts; do not force-kill around save errors.
