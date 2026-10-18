-- Runs in Hyprland's Lua runtime with a fake API; never changes live bindings.
return function(path)
    local bindings, calls = {}, {}
    local active, windows
    local fake = {
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
    cleanup()
    for _, binding in ipairs(bindings) do assert(binding.removed) end
    install()
    windows = { { class = "dev.mryll.waynote", visible = true } }
    bindings[4].callback()
    assert(calls[4] == "waynote preview", "Installed integration uses the normal app")
end
