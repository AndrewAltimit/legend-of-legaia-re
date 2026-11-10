-- autorun_pause_hud_fade.lua
--
-- Does the field party HUD stay on screen while the pause wipe darkens the
-- field into the menu? Loads a retail field state, waits for the idle
-- countdown to bring the HUD up, presses the menu button, and logs per vsync
-- what the HUD routine FUN_801D0D38 (PROT 0897, base 0x801CE818) did:
--   * 0x801D0D38  entry (counted per vsync)
--   * 0x801D0DB8  suppress exit (the gate words: _DAT_8007B868, the Field HP
--                 Display option 0x800845C4 == 2, a held D-pad bit, the
--                 menu lock _1F800394 & 0x08000000)
--   * 0x801D0F20  panel build (countdown reached zero)
-- with the gate words, the countdown 0x801F348C, the player's +0x10, the
-- game mode, the screen fade level _DAT_8007B440 and the menu accept
-- (0x801D02E8). Every BP is guarded on the 0897 instruction word at its VA.
--
-- Output: <LEGAIA_OUT_DIR>/hud.log, optional shot_<v>.raw at LEGAIA_SHOTS.
-- Env: LEGAIA_SSTATE / LEGAIA_FRAMES / LEGAIA_OUT_DIR (run_probe.sh),
--      LEGAIA_PRESS ("240:START"), LEGAIA_SHOTS ("238,241,244").
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")
local bp = require("probe.bp")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 330)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "captures/pause_hud_fade")
local PRESS = probe.getenv("LEGAIA_PRESS", "240:START")
local SHOTS = probe.getenv("LEGAIA_SHOTS", "")
os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/hud.log", "w")
local function log(s)
    PCSX.log("[hud] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end
local function u32(x) return (tonumber(x) or 0) % 0x100000000 end

local W = {
    hud = { 0x801D0D38, 0x3C028008 },
    suppress = { 0x801D0DB8, 0x08074C45 },
    build = { 0x801D0F20, 0x24150010 },
    accept = { 0x801D02E8, 0x0C00D6D4 },
}
local function is_0897(k) return u32(probe.read_u32(W[k][1])) == W[k][2] end

local vs = 0
local n = { hud = 0, suppress = 0, build = 0 }
local presses = {}
for at, name in string.gmatch(PRESS, "(%d+):(%u+)") do
    presses[#presses + 1] = { at = tonumber(at), btn = pad.BTN[name], name = name }
end
local shots = {}
for v in string.gmatch(SHOTS, "(%d+)") do shots[tonumber(v)] = true end
local HOLD = 6

local function pflags()
    local p = u32(probe.read_u32(0x8007C364))
    if p >= 0x80000000 and p < 0x80200000 then return u32(probe.read_u32(p + 0x10)) end
    return 0
end

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
        bp.arm(W.hud[1], "Exec", 4, "hud_entry", function()
            if is_0897("hud") then n.hud = n.hud + 1 end
        end)
        bp.arm(W.suppress[1], "Exec", 4, "hud_suppress", function()
            if is_0897("suppress") then n.suppress = n.suppress + 1 end
        end)
        bp.arm(W.build[1], "Exec", 4, "hud_build", function()
            if is_0897("build") then n.build = n.build + 1 end
        end)
        bp.arm(W.accept[1], "Exec", 4, "menu_accept", function()
            if is_0897("accept") then log(string.format("v=%d ACCEPT menu", vs)) end
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
            "v=%d mode=%02X hud=%d suppress=%d build=%d count=%d b868=%08X opt=%d dpad=%04X sys=%08X pflags=%08X fade=%d",
            tick, probe.read_u8(0x8007B83C) or 0, n.hud, n.suppress, n.build,
            (probe.read_u16(0x801F348C) or 0), u32(probe.read_u32(0x8007B868)),
            u32(probe.read_u32(0x800845C4)), u32(probe.read_u32(0x8007B850)) % 0x10000,
            u32(probe.read_scratch_u32(0x1F800394)), pflags(),
            u32(probe.read_u32(0x8007B440))))
        n.hud, n.suppress, n.build = 0, 0, 0
    end,
    on_done = function()
        for _, s in ipairs(presses) do pad.release(s.btn) end
        log("=== pause_hud_fade done ===")
        if LOG then LOG:close() end
    end,
})
