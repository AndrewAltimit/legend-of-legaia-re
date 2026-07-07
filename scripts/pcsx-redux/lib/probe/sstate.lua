-- probe/sstate.lua  -- save-state load/save wrapper.
--
-- PCSX-Redux's PCSX.loadSaveState does NOT auto-decompress; gzipped
-- save states (what the GUI writes) have to be wrapped in zReader so
-- the decompressed stream is what hits loadSaveState. Probe autosaves
-- (M.save below) are written RAW because the Lua API exposes no
-- zWriter. M.load sniffs the gzip magic (1f 8b) and handles both.
--
-- Returns true on success, false if the file can't be opened. Logs the
-- failure to PCSX.log so the probe driver can decide whether to bail.
--
-- Usage:
--   local sstate = require("probe.sstate")
--   if not sstate.load("/path/to/state.sstate7") then PCSX.quit(2) end
--   sstate.save("/path/to/autosave.sstate")

local M = {}

local function is_gzip(path)
    local fh = io.open(path, "rb")
    if fh == nil then return false end
    local magic = fh:read(2)
    fh:close()
    return magic == "\031\139" -- 0x1f 0x8b
end

function M.load(path)
    local gz = is_gzip(path)
    local fh, err = Support.File.open(path, "READ")
    if fh == nil or fh:failed() then
        PCSX.log(string.format("[probe] FATAL: cannot open %s (%s)",
            path, tostring(err)))
        return false
    end
    if gz then
        local zfh = Support.File.zReader(fh)
        PCSX.loadSaveState(zfh)
        zfh:close()
    else
        PCSX.loadSaveState(fh)
    end
    fh:close()
    return true
end

-- Write the current emulator state to `path` (raw, uncompressed).
-- Returns true on success. Call from a vsync handler only.
function M.save(path)
    local ok, slice = pcall(PCSX.createSaveState)
    if not ok or slice == nil then
        PCSX.log(string.format("[probe] autosave FAILED (createSaveState): %s",
            tostring(slice)))
        return false
    end
    local fh, err = Support.File.open(path, "TRUNCATE")
    if fh == nil or fh:failed() then
        PCSX.log(string.format("[probe] autosave FAILED: cannot open %s (%s)",
            path, tostring(err)))
        return false
    end
    fh:writeMoveSlice(slice)
    fh:close()
    return true
end

return M
