-- autorun_pause_wipe_step.lua
--
-- How many vsyncs does the pause menu's field wipe take each way, and on
-- which frame step? Loads a retail field state, presses the menu button,
-- backs out of the root list, and logs per vsync:
--   * every call of the wipe emitter FUN_8003479C (SCUS) with its level a0
--   * every pass of the pause-menu session FUN_801ED308 (field overlay,
--     guarded on its `lh v1,0x54(s0)` word) with the session phase +0x54
--   * the game mode 0x8007B83C, the frame step DAT_1F800393, the frame-step
--     floor DAT_8007B9D8, the brightness _DAT_8007B440 and the menu counter
--     _DAT_8007B43C.
--
-- The menu button is TRIANGLE and the root list's cancel CIRCLE.
--
-- Output: <LEGAIA_OUT_DIR>/wipe.log, optional shot_<v>.raw at LEGAIA_SHOTS.
-- Env: LEGAIA_SSTATE / LEGAIA_FRAMES / LEGAIA_OUT_DIR (run_probe.sh),
--      LEGAIA_PRESS ("130:TRIANGLE,300:CIRCLE"), LEGAIA_SHOTS ("149,160").
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")
local bp = require("probe.bp")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 300)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "captures/pause_wipe_step")
local PRESS = probe.getenv("LEGAIA_PRESS", "130:TRIANGLE,300:CIRCLE")
local SHOTS = probe.getenv("LEGAIA_SHOTS", "")
os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/wipe.log", "w")
local function log(s)
    PCSX.log("[wipe] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end
local function u32(x) return (tonumber(x) or 0) % 0x100000000 end

local SESSION = 0x801ED308
local SESSION_GUARD = { 0x801ED31C, 0x86030054 } -- lh v1,0x54(s0)
local EMITTER = 0x8003479C

local vs = 0
local emits = {}
local passes = {}
local presses = {}
for at, name in string.gmatch(PRESS, "(%d+):(%u+)") do
    presses[#presses + 1] = { at = tonumber(at), btn = pad.BTN[name], name = name }
end
local HOLD = 4
local shots = {}
for v in string.gmatch(SHOTS, "(%d+)") do shots[tonumber(v)] = true end

local function shot(v)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if ok and ss then
        local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
        local h = io.open(string.format("%s/shot_%d.raw", OUT_DIR, v), "wb")
        h:write(tostring(ss.data)); h:close()
        local m = io.open(string.format("%s/shot_%d.raw.meta", OUT_DIR, v), "w")
        m:write(string.format("width=%d\nheight=%d\nbpp=%d\n", tonumber(ss.width), tonumber(ss.height), bpp))
        m:close()
    end
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        bp.arm(EMITTER, "Exec", 4, "wipe_emit", function()
            local r = PCSX.getRegisters()
            emits[#emits + 1] = string.format("%d", u32(r.GPR.n.a0))
        end)
        bp.arm(SESSION, "Exec", 4, "pause_session", function()
            if u32(probe.read_u32(SESSION_GUARD[1])) ~= SESSION_GUARD[2] then return end
            local r = PCSX.getRegisters()
            local a0 = u32(r.GPR.n.a0)
            passes[#passes + 1] = string.format("%d", probe.read_u16(a0 + 0x54) or -1)
        end)
        return {}
    end,
    on_capture = function(_ctx, tick)
        vs = tick
        for _, s in ipairs(presses) do
            if tick == s.at then
                pad.force(s.btn)
                log(string.format("v=%d press %s", tick, s.name))
            elseif tick == s.at + HOLD then
                pad.release(s.btn)
            end
        end
        if shots[tick] then shot(tick) end
        log(string.format(
            "v=%d mode=%02X step=%d floor=%d fade=%d count=%d session=[%s] emit=[%s]",
            tick, probe.read_u8(0x8007B83C) or 0,
            probe.read_scratch_u32(0x1F800390) and
                math.floor(u32(probe.read_scratch_u32(0x1F800390)) / 0x1000000) % 0x100 or -1,
            u32(probe.read_u32(0x8007B9D8)),
            u32(probe.read_u32(0x8007B440)), u32(probe.read_u32(0x8007B43C)),
            table.concat(passes, ","), table.concat(emits, ",")))
        emits, passes = {}, {}
    end,
    on_done = function()
        for _, s in ipairs(presses) do pad.release(s.btn) end
        log("=== pause_wipe_step done ===")
        if LOG then LOG:close() end
    end,
})
