-- autorun_s3_pc.lua
--
-- Pinpoint the town01-opening deadlock instruction. Resumes the
-- s2_rimelm_town01 anchor and, in a window AFTER the ~510-frame auto-play (when
-- the opening cutscene has stalled), histograms the field-VM dispatcher args
-- (FUN_801DE840 = (a0 record_base, a1 pc, a2 ctx)). The parked cutscene context
-- re-enters the VM at the SAME (base, pc) every frame, so the dominant (a0,a1)
-- whose opcode is a wait/yield IS the instruction the opening waits on. a1 maps
-- directly to the `man-scripts --disasm-partition 2` (+offset) column.
--
-- Env: LEGAIA_SSTATE (resume), LEGAIA_OUT_DIR, LEGAIA_WIN_LO/HI (frame window).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_VM  = 0x801DE840 -- field-VM dispatcher; a0=base, a1=pc, a2=ctx
local FIELD_BP  = 0x8001698C -- per-frame field tick (frame clock)
local PLAYER    = 0x8007C364

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s3_pc")
local WIN_LO     = tonumber(env.getenv("LEGAIA_WIN_LO", "560")) or 560
local WIN_HI     = tonumber(env.getenv("LEGAIA_WIN_HI", "1300")) or 1300
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/pc.log", "w")
local function log(s) PCSX.log("[s3pc] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end

local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end

local vsync, loaded = 0, false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync = vsync + 1
    if not loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        loaded = true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
end)

local frame = 0
local hist = {}   -- "a0:a1" -> { n, a0, a1, op }
local recording = false

bp.arm(FIELD_VM, "Exec", 4, "field_vm", function()
    if not recording then return end
    local r = PCSX.getRegisters()
    local a0 = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
    local a1 = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
    if a0 < 0 then a0 = a0 + 0x100000000 end
    if a1 < 0 then a1 = a1 + 0x100000000 end
    local key = string.format("%08X:%X", a0, a1)
    local e = hist[key]
    if e == nil then
        local op = mem.in_ram(a0 + a1) and mem.read_u8(a0 + a1) or nil
        hist[key] = { n = 1, a0 = a0, a1 = a1, op = op }
    else
        e.n = e.n + 1
    end
end)

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame = frame + 1
    recording = (frame >= WIN_LO and frame <= WIN_HI)
    if frame == WIN_LO then
        local pp = ru32(PLAYER)
        log(string.format("recording window open at frame %d (player flags=%s)",
            frame, pp and string.format("0x%08X", ru32(pp+0x10) or 0) or "nil"))
    end
    if frame == WIN_HI + 1 then
        -- dump top (a0,a1) by count
        local arr = {}
        for _, e in pairs(hist) do arr[#arr+1] = e end
        table.sort(arr, function(a, b) return a.n > b.n end)
        log(string.format("=== top field-VM (base,pc) over frames %d..%d (%d distinct) ===",
            WIN_LO, WIN_HI, #arr))
        for i = 1, math.min(20, #arr) do
            local e = arr[i]
            log(string.format("  n=%-6d base=0x%08X pc=0x%04X op@pc=%s",
                e.n, e.a0, e.a1, e.op ~= nil and string.format("0x%02X", e.op) or "nil"))
        end
        if LOG then LOG:close() end
        PCSX.quit(0)
    end
end)

log("s3-pc armed; resume + histogram field-VM PCs at the stall")
