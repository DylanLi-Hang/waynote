-- Optional Hyprland Lua integration. Returns a cleanup function for these bindings only.
return function(command, app_class)
    command = command or "waynote preview"
    app_class = app_class or "dev.mryll.waynote"
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
    end
end
