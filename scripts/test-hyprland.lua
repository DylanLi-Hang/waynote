-- Runs in Hyprland's Lua runtime with a fake API; never changes live bindings.
return function(path)
    local bindings, calls, events, handles, moves = {}, {}, {}, {}, {}
    local active, windows
    local fake = {
        on = function(event, callback)
            local handle = { callback = callback, removed = false }
            function handle:remove() self.removed = true end
            events[event] = handle
            handles[#handles + 1] = handle
            return handle
        end,
        dsp = { window = {
            float = function(spec) return spec end,
            move = function(spec) return spec end,
        } },
        dispatch = function(spec) moves[#moves + 1] = spec end,
        get_active_window = function() return active end,
        get_windows = function() return windows end,
        exec_cmd = function(command) calls[#calls + 1] = command end,
        bind = function(key, callback, options)
            assert(options.release and options.non_consuming and options.ignore_mods and options.transparent)
            local binding = { key = key, callback = callback, removed = false }
            function binding:remove() self.removed = true end
            bindings[#bindings + 1] = binding
            return binding
        end,
    }
    local install = assert(loadfile(path, "t", { hl = fake, ipairs = ipairs }))()
    local cleanup = install("development preview", "note.dev")
    assert(#bindings == 3 and bindings[1].key == "mouse:272")
    active, windows = { class = "editor" }, { { class = "note.dev", visible = true } }
    assert(#calls == 0, "Hover/focus change alone must not notify Waynote")
    for _, binding in ipairs(bindings) do binding.callback() end
    assert(#calls == 3 and calls[1] == "development preview")
    active = { class = "note.dev" }
    bindings[1].callback()
    assert(#calls == 3, "Clicks in the note or its popovers keep Edit")
    active = nil
    bindings[1].callback()
    assert(#calls == 3, "No active window must not notify Waynote")
    active, windows = { class = "editor" }, { { class = "note.dev", visible = false } }
    bindings[1].callback()
    assert(#calls == 3, "A hidden/on-another-workspace note must not be notified")
    windows = { { class = "note", visible = true } }
    bindings[1].callback()
    assert(#calls == 3, "Development bindings must match the exact application ID")
    local note = { class = "note.dev", pid = 42, floating = true,
        at = { x = 210, y = 175 }, monitor = { name = "eDP-1" } }
    events["window.open"].callback(note)
    assert(#moves == 0, "First map must keep compositor placement")
    events["window.close"].callback(note)
    local remapped = { class = "note.dev", pid = 42, monitor = { name = "eDP-1" } }
    events["window.open"].callback(remapped)
    assert(#moves == 2 and moves[1].action == "enable")
    assert(moves[2].window == remapped and moves[2].x == 210 and moves[2].y == 175
        and moves[2].relative == false, "Hide/show must restore the last floating position")
    note.at = { x = 300, y = 250 }
    events["window.close"].callback(note)
    events["window.open"].callback(remapped)
    assert(#moves == 4 and moves[4].x == 300 and moves[4].y == 250,
        "Moving the note must replace the previous position")
    for _, other in ipairs({
        { class = "editor", pid = 42, monitor = { name = "eDP-1" } },
        { class = "note.dev", pid = 43, monitor = { name = "eDP-1" } },
        { class = "note.dev", pid = 42, monitor = { name = "other" } },
        { class = "note.dev", pid = 42 },
    }) do events["window.open"].callback(other) end
    assert(#moves == 4, "Other windows/processes/monitors must keep normal placement")
    events["window.close"].callback({ class = "editor" })
    events["window.open"].callback(remapped)
    assert(#moves == 6, "Unrelated windows must not clear the saved note position")
    note.floating = false
    events["window.close"].callback(note)
    events["window.open"].callback(remapped)
    assert(#moves == 6, "A tiled note must not restore an obsolete floating position")
    cleanup()
    for _, handle in ipairs(handles) do assert(handle.removed) end
    for _, binding in ipairs(bindings) do assert(binding.removed) end
    install()
    windows = { { class = "dev.mryll.waynote", visible = true } }
    bindings[4].callback()
    assert(calls[4] == "waynote preview", "Installed integration uses the normal app")
end
