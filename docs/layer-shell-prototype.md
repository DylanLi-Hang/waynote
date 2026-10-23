# Layer Shell migration and isolated development app

Branch: `feature/layer-shell-window`. Layer Shell is opt-in; the installed app
and its notes are unchanged until explicitly running the installed Layer Shell package.

## NixOS test package

`packages.<system>.layer-shell` enables the Cargo feature, links gtk4-layer-shell,
and wraps the binary with `WAYNOTE_LAYER_SHELL=1`. The default package remains
an ordinary GTK window. The installed Layer Shell app uses `dev.mryll.waynote`
and the normal note storage; the isolated runner retains its development ID/data.
Quit the previous installed instance before launching the new package.

Hyprland consumers should omit the ordinary window rule and call the Lua helper
with its third argument `true`. Send `preview-outside` through `org.gtk.Actions`
to `dev.mryll.waynote` at `/dev/mryll/waynote`, using `gdbus call --session`.
This does not auto-start the app when it is absent. Disable the development
bindings with `scripts/layer-hyprland disable` before testing system bindings.

## Native note interface

Run `./scripts/layer-dev` to build and show the real note interface with Layer
Shell. It supports the usual `show-all`, `hide-all`, `summon`, `preview`, `new`,
and `quit` commands. Notes, configuration and window state are isolated under
`target/layer-dev`. Its distinct app ID is
`dev.mryll.waynote.layershell.development`. Window movement itself does not use Hyprland IPC.
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

For click-outside Preview on Hyprland, run `./scripts/layer-hyprland enable`
after starting the test app. This reuses `packaging/hyprland.lua` with three
non-consuming left/right/middle release bindings. A D-Bus `preview-outside`
action reuses the existing save/Preview path only for a visible, unfocused note
with no open popup and the pointer outside its surface. Keyboard focus alone
is insufficient: an inside double-click release can arrive before OnDemand
focus is granted. Hover alone never sends this action. Layer surfaces are not
normal Hyprland window-list entries, so the app checks focus itself.
No raw input devices or additional device permissions are used.

Run `./scripts/layer-hyprland disable` to remove only these bindings. Re-enabling
first removes its old handles. This is a live-session integration: a Hyprland
config reload removes it, so enable again afterwards. It does not change Meta+N,
installed Waynote's bindings, or system configuration. Disable it when finishing
the trial. Other compositors retain explicit title/Escape Preview controls.


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

## Repeatable continuous-drag regression

Run `nix develop --command python3 scripts/test-layer-pointer.py` with `sway`
on PATH (or set `WAYNOTE_TEST_SWAY` to its executable). This builds a test-only
virtual pointer and runs `layer_gui_regressions` on a fresh 1280×900 headless
output. Its private D-Bus session has no service activation directories, so the
test cannot auto-start desktop portal services. It uses isolated note storage
and terminates its compositor afterwards; artifacts remain under `/tmp`.

The pointer test checks the rendered panel after every motion **before release**:
forward motion, reversal, edge clamping, recovery to the original position, then
no movement after release. Removing `drag_update` fails this regression even if
`drag_end` eventually puts the panel at the correct destination. The optional
`WAYNOTE_TEST_POINTER` block is skipped by GUI runs without the pointer helper;
use this runner for drag acceptance. Cross-screen release-transfer is covered by the separate example suite below;
it is not yet a feature of the installed application.


## Release-transfer regression suite

The manually confirmed demo is preserved as `examples/release-transfer.rs`.
It includes the production `Panel` source, so continuous dragging and input-region
changes are exercised without a stale duplicate implementation. Transfer-specific
code remains in the example; this does not enable cross-screen movement in the
installed app.

Run from the checkout:

```sh
nix develop --command python3 scripts/test-release-transfer.py
```

Requires Sway on PATH, or `WAYNOTE_TEST_SWAY` pointing to its executable. The runner
builds the example and private pointer helper, starts its own headless compositor
and D-Bus session without portal activation, and cleans up its child processes.
It never drives the real desktop pointer. Logs and geometry remain in printed
`/tmp/waynote-*` artifact directories. GTK criticals abort the test process.

| Regression | What must remain true |
| --- | --- |
| Continuous movement before release | Every forward/reverse motion changes the rendered panel while pressed; clamping recovers without grab-offset drift. |
| Small → large, far-right drop | Destination position uses the new output allocation, never the previous small-screen boundary. |
| Large → small after a far-right placement | The old GtkFixed child offset must not prevent the canvas from shrinking; placement completes within the test timeout. |
| Vertical, offset screens with reserved bars | 3440×1440 above 1920×1200 at (760,1440), with 40px top bars, preserves global coordinates and completes transfer. |
| Mixed scale | A 1.5-scale second output keeps logical-coordinate placement correct. |
| Repeated round trip | Transfer, transfer back, and transfer again all settle; subsequent dragging still follows the pointer. |
| Release cleanup | Hover after release does not move the panel; remapping during release must not produce GTK criticals. |

Equal-size, unequal-width, mixed-scale, and vertical-with-bars scenarios run in
separate compositor instances. These tests preserve the failures found during
manual testing; they do not establish hotplug or arbitrary-compositor support.
