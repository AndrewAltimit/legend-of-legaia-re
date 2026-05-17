-- probe/csv.lua  -- auto-flushed CSV writer.
--
-- Every Csv:row call flushes immediately so an early process exit
-- (e.g. PCSX-Redux closed mid-capture, segfault inside a probe
-- callback) still leaves a partial-but-usable artefact on disk.
--
-- Usage:
--   local csv = require("probe.csv")
--   local f = csv.open("/tmp/out.csv", "vsync,pc,note")
--   f:row("%d,0x%08X,%s", vsync, pc, "hit")
--   f:close()

local M = {}

local Csv = {}
Csv.__index = Csv

function M.open(path, header)
    local fh, err = io.open(path, "w")
    if not fh then
        PCSX.log(string.format("[probe] FATAL: cannot open csv %s (%s)",
            path, tostring(err)))
        return nil
    end
    fh:write(header)
    if not header:find("\n$") then fh:write("\n") end
    fh:flush()
    return setmetatable({ fh = fh, path = path }, Csv)
end

function Csv:row(fmt, ...)
    if not self.fh then return end
    self.fh:write(string.format(fmt, ...))
    if not fmt:find("\n$") then self.fh:write("\n") end
    self.fh:flush()
end

function Csv:close()
    if self.fh then
        self.fh:flush()
        self.fh:close()
        self.fh = nil
    end
end

-- Exposed for callers that want to manually instantiate via setmetatable
-- (e.g. wrapping an existing file handle in tests).
M.Csv = Csv

return M
