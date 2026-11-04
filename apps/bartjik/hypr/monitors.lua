-- Load confirmed, per-user display preferences after the normal monitor rules.
-- The backend writes one atomic data file; previews never touch this file.
local config = os.getenv("XDG_CONFIG_HOME") or (os.getenv("HOME") .. "/.config")
local path = config .. "/bartjik/monitors.lua"
local file = io.open(path, "r")
if file then
    file:close()
    local rules = dofile(path)
    for _, rule in ipairs(rules) do
        hl.monitor(rule)
    end
end
