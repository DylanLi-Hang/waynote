# Waynote

A single-window Markdown notes board for Linux, forked from
[mryll/waynote](https://github.com/mryll/waynote).

- **Meta+N shows or hides the whole board.** A visible board always hides,
  including when another app has focus. It stays running while hidden.
- **Notes ▾** searches and reveals existing notes; **+ New note** creates a card.
- Click an unlocked note to edit its Markdown. Leaving it saves and returns to
  Preview. Typing is autosaved; failed saves keep the draft and show an error.
- Each card's **⋯** menu contains Copy, Color, Lock and Delete. Lock makes content
  read-only; selection, links and copying remain available.
- Markdown includes tables, task checkboxes, links, code and local images.
  Checkboxes persist immediately without entering Edit mode.
- Notes remain plain `.md` files in `$XDG_DATA_HOME/waynote/notes/`
  (normally `~/.local/share/waynote/notes/`). External edits refresh live;
  conflicting edits produce a conflict copy. Deleted files go to the sibling
  `trash/` directory.

The former per-note layers, pinning, desktop mode, roll-up, positioning and tray
are removed. Old metadata remains in existing files as unknown frontmatter;
new notes do not write those fields.

## Try without installing

On NixOS:

```sh
./scripts/dev run --bind
```

This builds the checkout, starts a separate development instance with test notes,
and temporarily assigns Meta+N on this machine's Hyprland Lua session. No system
switch is needed. See [development](docs/development.md) for checks and recovery.

For a normal build, install Rust and GTK 4 development libraries, then run
`cargo build --release`. With Nix, use `nix build` or `nix run` from this checkout.
Upstream distribution packages still contain the original interface.

## Desktop shortcut and window behavior

Bind a single shortcut to `waynote toggle`. Cold invocation starts the resident
app; further invocations use the same instance. The first invocation creates an
editable note if none exist. Closing the window hides it; `waynote quit` exits.

On Hyprland with Lua config:

```lua
hl.bind("SUPER + N", hl.dsp.exec_cmd("waynote toggle"))
hl.window_rule({ match = { class = "^dev[.]mryll[.]waynote$" }, float = true })
```

For a traditional Hyprland config:

```ini
bind = SUPER, N, exec, waynote toggle
windowrule = float, class:^(dev\.mryll\.waynote)$
```

The board is a regular GTK window: drag its header or use the compositor's move
and resize controls. Floating windows appear above tiled windows on Hyprland;
stacking relative to other floating windows is controlled by the compositor.
On KDE, create a window rule matching application ID `dev.mryll.waynote`, set
**Keep above** as desired, and assign Meta+N to `waynote toggle` in Shortcuts.
KDE has not been verified in this development session.

Other commands are `new`, `summon`, `show-all`, `hide-all` and `quit`.
Only `toggle` is needed for everyday use; creation is available in the toolbar.

## Development

[Developing and testing on NixOS](docs/development.md) documents the isolated
runner. The implementation consists of a GTK board/controller, Markdown parser
and renderer, and filesystem persistence/watcher. There is no layer-shell or
tray dependency.
