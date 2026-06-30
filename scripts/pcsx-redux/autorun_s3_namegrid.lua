-- autorun_s3_namegrid.lua
--
-- Recon the name-entry screen layout at the town01 stall, to plan the pad
-- timeline that completes it. Reads the 7x17 charset grid (0x801F29F0, 119
-- bytes), the live cursor index (0x8007BB88), and the working name buffer
-- (0x801F2A6C) once the name-entry SM (FUN_801F03F0) is active. Locates the
-- control-row sentinels (Space 0x64, End 0x65, Backspace 0x66) + separators
-- (0x7C) so the navigation path to End is computable.
--
-- Env: LEGAIA_SSTATE (resume), LEGAIA_OUT_DIR, LEGAIA_AT (frame to sample).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP = 0x8001698C
local GRID     = 0x801F29F0 -- charset grid, 119 bytes (17 cols x 7 rows)
local CURSOR   = 0x8007BB88 -- live cursor index (wrap mod 0x77)
local NAMEBUF  = 0x801F2A6C -- working name buffer
local SR_STATE = 0x8007B450

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s3_grid")
local AT         = tonumber(env.getenv("LEGAIA_AT", "800")) or 800
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/grid.log", "w")
local function log(s) PCSX.log("[grid] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru16(a) return mem.in_ram(a) and ((ru8(a) or 0) + 0x100*(ru8(a+1) or 0)) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end

local vsync, loaded = 0, false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync = vsync + 1
    if not loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        loaded = true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
end)

local frame, done = 0, false
bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame = frame + 1
    if frame ~= AT or done then return end
    done = true
    log(string.format("sample @frame %d  cursor=0x%s  _DAT_8007B450=0x%08X",
        frame, ru16(CURSOR) ~= nil and string.format("%04X", ru16(CURSOR)) or "nil",
        ru32(SR_STATE) or 0))
    -- dump the 119-cell grid as 7 rows x 17 cols
    log("grid (17 cols x 7 rows):")
    for row = 0, 6 do
        local cells = {}
        for col = 0, 16 do
            local b = ru8(GRID + row*17 + col)
            cells[#cells+1] = b ~= nil and string.format("%02X", b) or "--"
        end
        log(string.format("  row %d (idx %3d): %s", row, row*17, table.concat(cells, " ")))
    end
    -- locate sentinels + separators
    for _, t in ipairs({{0x64,"Space"},{0x65,"End"},{0x66,"Backspace"}}) do
        for i = 0, 118 do
            if ru8(GRID + i) == t[1] then
                log(string.format("  %s (0x%02X) at idx %d (row %d col %d)", t[2], t[1], i, math.floor(i/17), i%17))
            end
        end
    end
    -- working name
    local nm = {}
    for i = 0, 15 do
        local b = ru8(NAMEBUF + i) or 0
        if b == 0 then break end
        nm[#nm+1] = string.format("%02X", b)
    end
    log("name buffer bytes: " .. table.concat(nm, " "))
    if LOG then LOG:close() end
    PCSX.quit(0)
end)

log("s3 namegrid recon armed")
