# Layer Shell migration: first validation gate

Branch: `feature/layer-shell-window`. The first gate is an isolated GTK4 Layer
Shell window, not a migration of the installed note application. The ordinary
`waynote` binary and real note store keep their current behavior.

## Run and stop

```sh
./scripts/layer-prototype          # build, then show/hide the resident prototype
./scripts/layer-prototype show
./scripts/layer-prototype hide
./scripts/layer-prototype preview
./scripts/layer-prototype quit
./scripts/layer-prototype smoke    # mapping/mode/save/remap assertions; exits
```

The `smoke` command uses a separate application ID ending in `.smoke` and
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

File dialog tests a native chooser and its parenting only; selected files are
not read or imported. Search tests a focusable popup only, not real notes.
Dragging, resizing, position persistence, monitor selection, real note editing,
and clipboard/selection integration are intentionally deferred to later gates.

## Acceptance checklist

Automated smoke coverage:

- Window maps as a Layer Shell surface on Top with keyboard mode None.
- Preview is not active after initial map or hide/remap.
- Edit requests OnDemand; saving returns to None.
- Scratch saves and window visibility transitions succeed.

Interactive checks still required before migrating the real controller:

- Type in another app while Preview is visible; no keys reach the note.
- Enter Edit, type, then click another app; confirm save/Preview and focus return.
- Open Search, type, close it; confirm keyboard mode is released.
- Open/cancel the file chooser and inspect focus return.
- Switch normal/special workspaces and test against other floating/fullscreen windows.
- Check both monitors and scale factors; do not claim hotplug support yet.

## Later gates

1. Integrate the validated window/focus policy with the existing controller,
   retaining save-error, draft, menu and note-switching behavior.
2. Implement application-managed drag/resize, display choice and safe layout recovery.
3. Update the development runner and remove obsolete Hyprland helpers only after
   the replacement has passed interactive regression checks.
4. Trial against isolated copied fixtures before a deliberate installed-app migration.
