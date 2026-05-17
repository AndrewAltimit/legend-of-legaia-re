-- probe/sstate.lua  -- save-state load wrapper.
--
-- PCSX-Redux's PCSX.loadSaveState does NOT auto-decompress; gzipped
-- save states have to be wrapped in zReader so the decompressed stream
-- is what hits loadSaveState. This module hides that detail.
--
-- Returns true on success, false if the file can't be opened. Logs the
-- failure to PCSX.log so the probe driver can decide whether to bail.
--
-- Usage:
--   local sstate = require("probe.sstate")
--   if not sstate.load("/path/to/state.sstate7") then PCSX.quit(2) end

local M = {}

function M.load(path)
    local fh, err = Support.File.open(path, "READ")
    if fh == nil or fh:failed() then
        PCSX.log(string.format("[probe] FATAL: cannot open %s (%s)",
            path, tostring(err)))
        return false
    end
    local zfh = Support.File.zReader(fh)
    PCSX.loadSaveState(zfh)
    zfh:close()
    fh:close()
    return true
end

return M
