# Developing on NixOS

Build and run the checkout without installing it or switching the system:

```sh
./scripts/dev run
```

The script enters the flake's development shell, performs an incremental debug
build, and starts the app in the foreground. Run it again to rebuild and replace
the previous development session. Ctrl+C or `./scripts/dev stop` stops it.

The development app uses the separate D-Bus ID `dev.mryll.waynote.development`
and layer-shell namespace `waynote-dev`. Its notes, layout, configuration, and
logs live under `target/dev/`, separate from the installed app. The first run
creates a blue `[DEV] Waynote` sample note with task checkboxes. Subsequent runs
preserve edited test notes and config. `target/` is ignored by Git.

The current application still requires explicit edit commits: use Esc or its
Save control before restarting the development process. The agreed autosave,
focus-driven Edit/Preview, and single-window board are future work.

## Trying Meta+N on Hyprland

```sh
./scripts/dev run --bind
```

While this process is alive, Meta+N drives the development app. The existing
binding is restored when the app exits or the runner receives Ctrl+C/SIGTERM.
This changes only the running compositor, not NixOS or Home Manager files.

The binding helper targets Hyprland's Lua configuration API. For a Lua binding,
it reads the original single-line `hl.bind("SUPER + N", hl.dsp.exec_cmd(...), ...)`
from `$XDG_CONFIG_HOME/hypr/hyprland.lua` (normally `~/.config/hypr/hyprland.lua`).
It refuses ambiguous bindings instead of guessing how to restore them. Run
without `--bind` on other desktops or unsupported configurations.

If the runner is killed with SIGKILL, use `./scripts/dev stop` in the same
Hyprland session to recover the saved binding. A compositor config reload or a
new binding installed by the user takes precedence over automatic restoration.

You can also control only the running development instance explicitly:

```sh
./scripts/dev toggle
./scripts/dev new
./scripts/dev status
./scripts/dev stop
```

The installed app can continue running. Development commands use the separate
application ID; the normal installed `waynote` command still targets the
installed instance.

## Verification

```sh
./scripts/dev check       # runner regressions, Nix derivation evaluation, Rust tests, Clippy
./scripts/dev gui         # existing GTK/Wayland GUI regressions with temporary test data
./scripts/dev package     # build the wrapped Nix package without installing or activating
```

The first build downloads the pinned Nix dependencies and Cargo crates. Later
debug builds are incremental. The GUI suite requires an active Wayland session
with layer-shell support and briefly creates test surfaces. It currently checks
color changes while editing, external-file conflicts, and Wayland input-region
remapping. It does not yet test the planned board or focus-driven editing.

Use the real compositor to verify keyboard focus, Meta+N, stacking, and pointer
pass-through. A Hyprland result does not establish KDE compatibility. Application
output is appended to `target/dev/app.log`; saved test notes are under
`target/dev/data/waynote/notes/`.
