# Waynote

A lightweight Markdown sticky note for Linux, forked from
[mryll/waynote](https://github.com/mryll/waynote).

- **Meta+N shows or hides the window.** A visible window always hides,
  including when another app has focus. It stays running while hidden.
- The header's **Notes** button opens a searchable selection panel. One note is
  visible at a time; switching saves the current draft and opens the selected note in Preview.
  Each entry shows a left-aligned title and a separate body summary; long text is ellipsized.
  **＋** creates and selects a note, ready to type.
- Double-click an unlocked note to edit its Markdown; a single click stays in Preview.
  Mouse movement and window focus changes keep Edit active. With the optional
  Hyprland outside-click integration, clicking another window saves and returns
  to Preview while the note stays visible.
  Click the paper's title or press Esc to save and
  return to Preview; Esc in Preview hides the window. Esc closes an open panel
  first. Hiding also saves. Typing is autosaved; failed saves keep the draft in
  Edit and show an error.
- The note's **⋯** menu contains Copy, Color, Lock and Delete. Lock makes content
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
Resizing is remembered across hide/show and app restarts.

On Hyprland with Lua config:

```lua
hl.bind("SUPER + N", hl.dsp.exec_cmd("waynote toggle"))
hl.window_rule({ match = { class = "^dev[.]mryll[.]waynote$" }, float = true, no_anim = true })
```

The rule disables animations for Waynote so restoring its position appears
instantly instead of sliding from the compositor’s initial placement.

To keep the floating position across hide/show and save/return to Preview when
clicking another window, load the optional Lua helper after these rules (use its
absolute path):

```lua
dofile("/path/to/waynote/packaging/hyprland.lua")()
```

Nix packages install the helper at `$out/share/waynote/hyprland.lua`.
It observes left/right/middle button releases without consuming clicks, and
ignores hover, note-internal clicks and popovers. It only notifies a visible
Waynote window. It also remembers a floating note’s position on unmap and restores
it when the same process remaps on the same monitor. A new process or different
monitor uses normal compositor placement; the position is kept only for this
Hyprland configuration session. For pinned notes, hiding returns focus to the
visible special workspace on the same monitor so it stays open. Hiding an
unfocused note does not change focus. Reloading Hyprland removes the bindings and event
handlers if the helper is removed from the config. `scripts/dev run --bind` installs/removes them automatically
for the isolated development app. This integration requires Hyprland's Lua API.

For a traditional Hyprland config:

```ini
bind = SUPER, N, exec, waynote toggle
windowrule = float, class:^(dev\.mryll\.waynote)$
```

The paper fills a regular GTK window, with all controls in its own header and
no surrounding application frame. Drag the title or use the compositor's move
and resize controls. Floating windows appear above tiled windows on Hyprland;
stacking relative to other floating windows is controlled by the compositor.
On KDE, create a window rule matching application ID `dev.mryll.waynote`, set
**Keep above** as desired, and assign Meta+N to `waynote toggle` in Shortcuts.
KDE has not been verified in this development session.

Other commands are `new`, `summon`, `show-all`, `hide-all`, `preview` and `quit`.
`preview` saves and leaves Edit without showing, hiding or focusing the window;
it does nothing when Waynote is not running. Failed saves retain Edit and the draft.
Only `toggle` is needed for everyday use; creation is available in the note header.

## Development

[Developing and testing on NixOS](docs/development.md) documents the isolated
runner. The implementation consists of a GTK window/controller, Markdown parser
and renderer, and filesystem persistence/watcher. There is no layer-shell or
tray dependency.
