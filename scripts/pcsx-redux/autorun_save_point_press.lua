-- autorun_save_point_press.lua
--
-- Does a field save point open the pause menu by itself, and what resumes the
-- script afterwards?
--
-- Retail's save point is a one-record script, `31 02` then a loop on
-- `49 01 00` (field-VM op 0x49, sub-op 1). The op's Idle arm spawns the
-- subsystem actor the menu button spawns (FUN_80020DE0(0x8007065C, ..) at
-- 0x801E09A0) and parks the operand pointer in _DAT_8007B450 (0x801E09A8).
-- That actor's enter half FUN_801F1278 installs handler 7 before it reads the
-- sub-op table, and handler 7 (FUN_801F1F4C) picks 0x30, the pause-menu
-- session FUN_801ED308, whenever the park is live. The dispatcher's retire arm
-- writes the Done sentinel 1 (0x801F16AC) only while the park is non-zero.
--
-- The probe loads a state standing at a save point, presses Cross in each
-- facing until the park is armed, then lets the menu run, backing out with
-- Circle, and logs per vsync the game mode, the park word and the subsystem
-- actor's handler id, plus an exec hit on each address above.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT (CSV, default captures/save_point_press.csv),
--      LEGAIA_FRAMES (default 2400).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local pad   = require("probe.pad")

local OUT    = probe.getenv("LEGAIA_OUT", "captures/save_point_press.csv")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)
local PARK   = 0x8007B450
local MODE   = 0x8007B83C

os.execute(string.format("mkdir -p %q", OUT:match("(.*/)") or "."))
local csv = probe.csv_open(OUT, "vsync,event,mode,park,pc,ra,a0_50")
local function row(v, ev, pc, ra, a050)
    csv:row("%d,%s,%d,0x%08X,0x%08X,0x%08X,%s", v, ev,
        mem.read_u8(MODE) or -1, mem.read_u32(PARK) or 0, pc or 0, ra or 0, a050 or "")
end

local vnow = 0
local HITS = {
    { 0x801E09A8, "park_store" },      -- op 0x49 Idle arm: park = operand
    { 0x801E08D8, "done_consume" },    -- op 0x49 Done arm: park = 0
    { 0x801F1278, "enter_1278" },      -- subsystem actor enter half
    { 0x801F1F4C, "state_pick_7" },    -- handler 7
    { 0x801ED308, "pause_session_30" },-- handler 0x30
    { 0x801F16AC, "retire_done_store" },-- retire arm, park != 0 -> 1
    { 0x801F1688, "retire_park_zero" },-- retire arm, park == 0
}

local phase, press_i, armed_at, last_park = "PRESS", 0, nil, 0
local DIRS = { pad.BTN.UP, pad.BTN.RIGHT, pad.BTN.DOWN, pad.BTN.LEFT }
local held = nil
local function hold(b) if held ~= b then if held then pad.release(held) end; pad.force(b); held = b end end
local function release() if held then pad.release(held); held = nil end end

probe.run({
    sstate         = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    on_arm = function()
        local descs = {}
        for _, h in ipairs(HITS) do
            local d = { addr = h[1], name = h[2], hits_ref = { n = 0 } }
            probe.arm_breakpoint(h[1], "Exec", 4, h[2], function()
                d.hits_ref.n = d.hits_ref.n + 1
                if d.hits_ref.n <= 40 then
                    local r = PCSX.getRegisters()
                    local a0 = tonumber(r.GPR.n.a0)
                    local a050 = ""
                    if mem.in_ram(a0) then a050 = string.format("0x%04X", mem.read_u16(a0 + 0x50) or 0) end
                    row(vnow, h[2], tonumber(r.pc), tonumber(r.GPR.n.ra), a050)
                end
            end)
            descs[#descs + 1] = d
        end
        return descs
    end,
    on_capture = function(ctx, v)
        vnow = v
        local park = mem.read_u32(PARK) or 0
        if park ~= last_park then row(v, "park_change"); last_park = park end
        if v % 30 == 0 then row(v, "poll") end
        if phase == "PRESS" then
            if park ~= 0 then
                phase = "WATCH"; armed_at = v; release()
                row(v, "armed")
                return
            end
            -- 20-vsync cycle: tap a facing, then Cross.
            local t = v % 40
            local d = DIRS[(math.floor(v / 40) % 4) + 1]
            if t < 3 then hold(d) elseif t >= 20 and t < 23 then hold(pad.BTN.CROSS) else release() end
            if v > FRAMES - 600 then phase = "WATCH" end
        elseif phase == "WATCH" then
            -- Let the menu open, then back out with Circle every 45 vsyncs.
            local t = (v - (armed_at or v)) % 45
            if v - (armed_at or v) > 300 and t < 3 then hold(pad.BTN.CIRCLE) else release() end
        end
    end,
    on_done = function() release(); csv:close() end,
})
