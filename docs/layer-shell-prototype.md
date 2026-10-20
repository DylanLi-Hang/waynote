# Layer Shell migration and isolated development app

Branch: `feature/layer-shell-window`. Layer Shell is opt-in; the installed app
and its notes have not been migrated.

## Native note interface

Run `./scripts/layer-dev` to build and show the real note interface with Layer
Shell. It supports the usual `show-all`, `hide-all`, `summon`, `preview`, `new`,
and `quit` commands. Notes, configuration and window state are isolated under
`target/layer-dev`. Its distinct app ID is
`dev.mryll.waynote.layershell.development`. The runner does not use Hyprland IPC.
The optional Cargo feature retains its initial name `layer-shell-prototype`;
the application additionally requires `WAYNOTE_LAYER_SHELL=1` to enable it.

The header follows the pointer continuously while dragging, with a bottom-right
resize grip and explicit Hide button. A transparent Layer Shell surface stays
anchored to all four edges of the current output; only the note panel moves
inside a GtkFixed. Surface-relative pointer coordinates avoid feedback from a
moving widget. The GDK input region follows the panel's actual allocation, so
outside clicks pass through. It starts empty on every map until layout is ready.
Each region change schedules a render so the final resize region is committed
even if the pointer stops and the note needs no further repaint.

Position and size persistence stores panel geometry, never the output-sized
surface. Drag and resize clamp to the current output; stored positions are
clamped after layout. Monitor choice, cross-monitor dragging and hotplug
migration remain unsupported/unverified. Very small outputs below the UI's
natural minimum size are not supported.
Preview uses keyboard mode None; editing and Notes/action popovers use OnDemand.
Focus loss attempts to save but retains Edit, including on desktops where focus
follows the pointer. Click the title or press Escape to return to Preview;
hiding or switching notes also finishes editing. Failed saves keep the draft.
No import or File dialog control is included in the note interface.

The ignored `layer_gui_regressions` Rust test exercises the real controller on
Wayland using temporary notes: edit/save, Notes search, action menu, keyboard
modes, failed-save draft retention, persisted panel dimensions/placement and
unfocused hide/remap. Two geometry unit tests cover drag reversals and recovery
from boundaries.

Real virtual-pointer tests have passed on an isolated headless Sway 1.12 output
(1280×900, scale 1): continuous/reverse dragging, all boundaries, recovery,
release, outside click-through to a second GTK window, inside click isolation,
resize, hide/remap and process restart with restored geometry. Pointer input was
sent only to the private test compositor. The normal-window GUI regression has also passed. These results do not establish cross-monitor or fractional
scale behavior, or input behavior on other compositors.

For private compositor tests, optional `WAYNOTE_GEOMETRY_LOG=/tmp/geometry`
records the actual panel rectangle (`x y width height`, logical coordinates).
It is disabled by default. Never inject test input into the working desktop.

## Isolated protocol prototype

## Run and stop

```sh
./scripts/layer-prototype          # build, then show/hide the resident prototype
./scripts/layer-prototype show
./scripts/layer-prototype hide
./scripts/layer-prototype preview
./scripts/layer-prototype quit
./scripts/layer-prototype smoke    # mapping/mode/save/remap assertions; exits
./scripts/layer-prototype dialog-smoke  # real chooser open/cancel; exits
```

The smoke commands use a separate application ID ending in `.smoke` and
a `smoke/` subdirectory; it never addresses the interactive prototype or installed Waynote.
The prototype is an opt-in Cargo example (`layer-shell-prototype` feature).
The script uses the pinned development shell, not a system build or activation.

The D-Bus ID and Layer Shell namespace are
`dev.mryll.waynote.layershell.prototype`. Scratch text lives in
`target/layer-prototype/data/draft.txt`; XDG roots are isolated beneath
`target/layer-prototype`. No real notes are imported. No shortcuts, compositor
rules or Hyprland IPC calls are installed. A compositor providing Layer Shell
v4 is required for on-demand keyboard focus; unsupported sessions fail clearly.

## Behavior to evaluate

The window requests the Top layer, reserves no screen space, and initially
anchors near the top-right of the compositor-selected monitor. Top means above
normal workspace windows, not above every fullscreen or overlay surface.
Preview requests no keyboard focus. Edit (or double-clicking the scratch text)
and the Search popup request on-demand focus, never exclusive focus. Depending
on the compositor, entering Edit may require a subsequent click in the editor.

Preview saves the scratch text, then releases keyboard focus. Escape first
closes Search, then leaves Edit; Escape can only arrive while the surface has
keyboard focus. Use Hide or the CLI in non-focusable Preview. Losing active
keyboard focus while editing attempts save/Preview. This is a focus-loss policy,
not an equivalent of the old global outside-click observer: it may also react
to keyboard window switches. Save failures retain the draft and prevent hiding.

The CLI-only `dialog-smoke` test opens a standalone native chooser; selected files are not read or
imported. It deliberately has no GTK parent: exporting a Layer Shell surface
as an xdg_toplevel parent causes a fatal Wayland protocol error. Only one chooser
can open at a time. The dialog smoke test opens and cancels it, then checks the
callback, window survival and non-focusable Preview. Search tests a focusable popup only, not real notes.
The scratch prototype is retained for focused protocol regressions. The native
interface above now covers real note editing and basic geometry.

## Acceptance checklist

Automated smoke coverage:

- Window maps as a Layer Shell surface on Top with keyboard mode None.
- Preview is not active after initial map or hide/remap.
- Edit requests OnDemand; saving returns to None.
- Scratch saves and window visibility transitions succeed.

Interactive checks still required before migrating the installed application:

- Type in another app while Preview is visible; no keys reach the note.
- Enter Edit, type, then click another app; confirm save/Preview and focus return.
- Open Search, type, close it; confirm keyboard mode is released.
- Run `dialog-smoke` to check the standalone chooser regression.
- Switch normal/special workspaces and test against other floating/fullscreen windows.
- Check both monitors and scale factors; do not claim hotplug support yet.

## Later gates

1. Complete interactive controller checks, including save-error and draft recovery.
2. Validate additional compositors, scale factors, display choice and hotplug recovery.
3. Update the development runner and remove obsolete Hyprland helpers only after
   the replacement has passed interactive regression checks.
4. Trial against isolated copied fixtures before a deliberate installed-app migration.
