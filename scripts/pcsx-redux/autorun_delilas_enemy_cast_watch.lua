-- autorun_delilas_enemy_cast_watch.lua
--
-- Choreography recorder for a NATURAL enemy-side Delilas special cast:
-- load a retail savestate captured just before (or inside) the cast and
-- let it play out untouched. Per frame, log the battle phase band
-- (ctx+7), the module phase (ctx+0x279), the slot-B module word, and a
-- change-log of the caster's and victim's action/anim byte windows
-- (+0x1D8..+0x1F4) - the module stages caster clips and victim
-- reactions through these, so the change-log IS the choreography
-- timeline (which archive anim fires at which frame). Screenshots on a
-- cadence for visual correlation.
--
-- Env:
--   LEGAIA_SSTATE      savestate (required)
--   LEGAIA_FRAMES      capture frames (default 3600)
--   LEGAIA_CASTER      caster actor slot (default 3 = first monster seat)
--   LEGAIA_VICTIM      victim actor slot (default 0 = first party seat)
--   LEGAIA_SHOT_EVERY  screenshot cadence in frames (default 150)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 3600)
local CASTER = probe.getenv_num("LEGAIA_CASTER", 3)
local VICTIM = probe.getenv_num("LEGAIA_VICTIM", 0)
local SHOT_EVERY = probe.getenv_num("LEGAIA_SHOT_EVERY", 150)

local CTX_PTR = 0x8007BD24
local ACTOR_TABLE = 0x801C9370
local SLOTB = 0x801F69D8

local function u8(a) return probe.read_u8(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function ptr_ok(p) return p ~= nil and p >= 0x80000000 and p < 0x80200000 end

local CSV = probe.csv_open(probe.out_path("enemy_cast_watch.csv"),
    "tick,phase,modphase,note")

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data))
    fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 228,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
end

local function window(base)
    local t = {}
    for off = 0x1D8, 0x1F4 do t[#t + 1] = string.format("%02X", u8(base + off)) end
    return table.concat(t)
end

local last = { phase = -1, slotb = -1, caster = "", victim = "" }

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function() return {} end,
    on_capture = function(c, elapsed)
        local cx = u32(CTX_PTR)
        if not ptr_ok(cx) then return end
        local ph = u8(cx + 7)
        local mph = u8(cx + 0x279)
        local sb = u32(SLOTB)
        if ph ~= last.phase then
            CSV:row("%d,0x%02X,0x%02X,phase", elapsed, ph, mph)
            last.phase = ph
        end
        if sb ~= last.slotb then
            CSV:row("%d,0x%02X,0x%02X,slotB=0x%08X", elapsed, ph, mph, sb)
            last.slotb = sb
        end
        local ca = u32(ACTOR_TABLE + CASTER * 4)
        local va = u32(ACTOR_TABLE + VICTIM * 4)
        if ptr_ok(ca) then
            local w = window(ca)
            if w ~= last.caster then
                CSV:row("%d,0x%02X,0x%02X,caster[1D8..1F4]=%s", elapsed, ph, mph, w)
                last.caster = w
            end
        end
        if ptr_ok(va) then
            local w = window(va)
            if w ~= last.victim then
                CSV:row("%d,0x%02X,0x%02X,victim[1D8..1F4]=%s", elapsed, ph, mph, w)
                last.victim = w
            end
        end
        if elapsed % SHOT_EVERY == 0 then
            screenshot(string.format("watch_%05d_ph%02X", elapsed, ph))
        end
        if elapsed % 300 == 0 then
            CSV:row("%d,0x%02X,0x%02X,tick", elapsed, ph, mph)
        end
    end,
    on_done = function()
        CSV:row("0,0,0,DONE")
        CSV:close()
    end,
})
