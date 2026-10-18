-- Optional Hyprland Lua integration. Cleanup removes only its own handles.
return function(command, app_class)
    command = command or "waynote preview"
    app_class = app_class or "dev.mryll.waynote"
    local last_position
    local events = {
        hl.on("window.close", function(window)
            if window.class ~= app_class then return end
            last_position = window.floating and window.monitor and {
                pid = window.pid, monitor = window.monitor.name,
                x = window.at.x, y = window.at.y,
            } or nil
            -- Hyprland otherwise picks a normal-workspace window on unmap,
            -- which dismisses the special workspace beneath a pinned note.
            local special = window.pinned and window.monitor
                and hl.get_active_special_workspace(window.monitor)
            local target = special and special.last_window
            if hl.get_active_window() == window and target and target.visible then
                hl.dispatch(hl.dsp.focus({ window = target }))
            end
        end),
        hl.on("window.open", function(window)
            if window.class ~= app_class or not last_position
                or window.pid ~= last_position.pid or not window.monitor
                or window.monitor.name ~= last_position.monitor then return end
            hl.dispatch(hl.dsp.window.float({ window = window, action = "enable" }))
            hl.dispatch(hl.dsp.window.move({
                window = window, x = last_position.x, y = last_position.y, relative = false,
            }))
        end),
    }
    local bindings = {}
    for _, button in ipairs({ "mouse:272", "mouse:273", "mouse:274" }) do
        bindings[#bindings + 1] = hl.bind(button, function()
            -- Release runs after the clicked window has received focus, even
            -- with follow-mouse enabled. Hover alone never invokes this handler.
            local active = hl.get_active_window()
            if not active or active.class == app_class then return end
            for _, window in ipairs(hl.get_windows()) do
                if window.class == app_class and window.visible then
                    hl.exec_cmd(command)
                    return
                end
            end
        end, {
            release = true, non_consuming = true, ignore_mods = true, transparent = true,
            description = "Waynote: preview after outside click",
        })
    end
    return function()
        for _, binding in ipairs(bindings) do binding:remove() end
        for _, event in ipairs(events) do event:remove() end
    end
end
