-- autorun_fishing_hook_audio.lua
--
-- Fish from a state with the lure in the water
-- (minigame_fishing_retail_lure_cast) until a fish strikes, with the catch
-- film's closed-loop pad, logging the hook-music latch 0x801D92A4, the sound
-- staging pointer _DAT_8007B85C, the battle source 0x8007056C and the
-- category-5 mixer record every vsync, and dumping the staging buffer and the
-- mixer table on the frame the latch rises (docs/subsystems/minigame-fishing.md).
--
-- Pure retail play: pad only, no RAM writes. Quits 400 vsyncs after the hook.
--
-- Output (out dir): hook.csv, hook_base.bin / hook_mixer.bin (the strike's
-- frame), hook30_mixer.bin, pre_*.bin (at arm, before the state settles).
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh --headless \
--       --scenario minigame_fishing_retail_lure_cast --iso <scratch copy> \
--       --lua scripts/pcsx-redux/autorun_fishing_hook_audio.lua --frames 9000

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 9000)
local AFTER = probe.getenv_num("LEGAIA_AFTER", 900)
local EVERY = probe.getenv_num("LEGAIA_SHOT_EVERY", 4)
local HI = probe.getenv_num("LEGAIA_TENSION_HI", 0xB00)
local LO = probe.getenv_num("LEGAIA_TENSION_LO", 0x700)
local SAVE_AT = {}
for n in string.gmatch(probe.getenv("LEGAIA_SAVE_AT", "60,240,600"), "%d+") do
    SAVE_AT[tonumber(n)] = true
end

local STATE = 0x801D926C
local HOOKED = 0x801D91B4
local TENSION = 0x801D9168
local RECORD = 0x801D927C
local FISH = 0x801D91D0
local SUB = 0x801D91C8
local SEAT = 0x801D9294
local SPECIES = 0x801D91CC
local LINE_XY = 0x801D9154
local POWER = 0x801D9274


local LATCH = 0x801D92A4
local BASE_PTR = 0x8007B85C
local MIXER = 0x80091508
local csv = nil
local cross = false
local pump, tap = 0, 0
local hook_at = nil
local last_latch = nil
local function set_cross(on)
    if on and not cross then probe.pad_force(probe.BTN.CROSS) end
    if not on and cross then probe.pad_release(probe.BTN.CROSS) end
    cross = on
end
local function dump(stem, addr, len)
    local b = probe.read_bytes(addr, len)
    if b == nil then return end
    local fh = io.open(probe.out_path(stem), "wb"); fh:write(tostring(b)); fh:close()
end
probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        csv = io.open(probe.out_path("hook.csv"), "w")
        csv:write("vsync,state,hooked,latch,base,src_0x8007056C,m5_0,m5_4,m5_8\n")
        local base = probe.read_u32(BASE_PTR) or 0
        dump("pre_base.bin", base, 0x40000)
        dump("pre_mixer.bin", MIXER, 0x100)
        return {}
    end,
    on_capture = function(_, el)
        local st = probe.read_u32(STATE) or 0
        local hooked = probe.read_u32(HOOKED) or 0
        local tension = probe.read_u32(TENSION) or 0
        local power = probe.read_u32(POWER) or 0
        local latch = probe.read_u32(LATCH) or 0
        local base = probe.read_u32(BASE_PTR) or 0
        if st == 0xC then tap = (tap + 1) % 40; set_cross(tap < 4)
        elseif st == 0x14 then set_cross(power >= 0xF00 and not cross)
        elseif st ~= 0x19 then set_cross(false)
        elseif hooked ~= 0 then
            if cross and tension >= HI then set_cross(false)
            elseif not cross and tension < LO then set_cross(true) end
        else pump = (pump + 1) % 65; set_cross(pump < 25) end
        if last_latch ~= nil and latch ~= last_latch then
            PCSX.log(string.format("[latch] %d -> %d at vsync %d state 0x%X", last_latch, latch, el, st))
            if latch == 1 and hook_at == nil then
                hook_at = el
                dump("hook_base.bin", base, 0x40000)
                dump("hook_mixer.bin", MIXER, 0x100)
            end
        end
        last_latch = latch
        csv:write(string.format("%d,0x%X,%d,%d,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X\n", el, st, hooked,
            latch, base, probe.read_u32(0x8007056C) or 0, probe.read_u32(MIXER + 60) or 0,
            probe.read_u32(MIXER + 64) or 0, probe.read_u32(MIXER + 68) or 0))
        if hook_at ~= nil and el - hook_at == 30 then
            dump("hook30_mixer.bin", MIXER, 0x100)
        end
        if hook_at ~= nil and el - hook_at >= 400 then csv:close(); csv = nil; PCSX.quit(0) end
    end,
})
