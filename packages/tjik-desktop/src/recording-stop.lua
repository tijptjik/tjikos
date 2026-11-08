-- Installed only for the lifetime of a recorder worker. A release binding for
-- the modifier itself fires on a bare tap, not after another Super shortcut.
if _G.runtjikRecordingStop then
    for _, binding in ipairs(_G.runtjikRecordingStop.bindings) do binding:remove() end
end
local bindings = {}
for _, key in ipairs({'Super_L', 'Super_R'}) do
    table.insert(bindings, hl.bind('SUPER + ' .. key, hl.dsp.exec_cmd(stop_command), {
        release = true, non_consuming = true, submap_universal = true,
        description = 'Stop Runtjik recording',
    }))
end
_G.runtjikRecordingStop = { token = stop_token, bindings = bindings }
