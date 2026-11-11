-- autorun_fishing_catch_film.lua
--
-- Film a retail fishing catch from a state with the lure in the water
-- (minigame_fishing_retail_lure_cast): work the lure until a fish strikes,
-- fight it on the tension gauge, and once the driver reaches the catch
-- (state word 0x801D926C = 0x1E) film the landed fish's lift and the result
-- plate, logging the hooked-fish actor and the result globals every vsync.
--
-- Pure retail play: pad only, no RAM writes.
--
-- The pad is closed-loop on the fishing globals (docs/subsystems/
-- minigame-fishing.md):
--   * the idle shore (state 0xC): tap Cross; the power meter (0x14): press
--     Cross once the power word 0x801D9274 reaches 0xF00 (a full cast; a
--     short one lands inside the record 500 the bite needs);
--   * no fish on (0x801D91B4 == 0): pump Cross - 25 vsyncs held, 40 idle -
--     the cadence template 3 rhythm, which arms the bite;
--   * a fish on: hold Cross while the tension gauge 0x801D9168 is under
--     LEGAIA_TENSION_HI (default 0xB00), release it until under
--     LEGAIA_TENSION_LO (default 0x700).
--   * from the catch on: nothing pressed.
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES (default 9000), LEGAIA_AFTER (vsyncs to
-- film after the catch, default 900), LEGAIA_SHOT_EVERY (default 4),
-- LEGAIA_SAVE_AT ("60,240,600": raw save states that many vsyncs after the
-- catch), LEGAIA_TENSION_HI / LEGAIA_TENSION_LO.
--
-- Output (out dir): catch.csv (one row per vsync), shot_<vsync>.raw (+meta)
-- from the catch on, catch_t<n>.rawsstate.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh --headless \
--       --scenario minigame_fishing_retail_lure_cast \
--       --lua scripts/pcsx-redux/autorun_fishing_catch_film.lua --frames 9000

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

local function s16(v) if v >= 0x8000 then return v - 0x10000 end return v end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return false end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return false end
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 240,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
    return true
end

local csv = nil
local cross = false
local catch_at = nil
local pump = 0
local tap = 0

local function set_cross(on)
    if on and not cross then probe.pad_force(probe.BTN.CROSS) end
    if not on and cross then probe.pad_release(probe.BTN.CROSS) end
    cross = on
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_fishing_catch_film.lua",
            { sstate = SSTATE, frames = FRAMES, after = AFTER })
        csv = io.open(probe.out_path("catch.csv"), "w")
        csv:write("vsync,state,hooked,tension,record,sub,seat,species,fish,"
            .. "flags,x,y,z,t22,rx,ry,rz,scale,clip,line_x,line_y\n")
        return {}
    end,
    on_capture = function(_, el)
        local st = probe.read_u32(STATE) or 0
        local hooked = probe.read_u32(HOOKED) or 0
        local tension = probe.read_u32(TENSION) or 0
        if catch_at == nil and st == 0x1E then
            catch_at = el
            PCSX.log(string.format("[catch] state 0x1E at vsync %d", el))
        end
        local power = probe.read_u32(POWER) or 0
        if catch_at ~= nil then
            set_cross(false)
        elseif st == 0xC then
            -- The idle shore: tap Cross to start the wind-up.
            tap = (tap + 1) % 40
            set_cross(tap < 4)
        elseif st == 0x14 then
            -- The power meter: lock it near the top (a fresh press edge).
            set_cross(power >= 0xF00 and not cross)
        elseif st ~= 0x19 then
            set_cross(false)
        elseif hooked ~= 0 then
            if cross and tension >= HI then set_cross(false)
            elseif not cross and tension < LO then set_cross(true) end
        else
            pump = (pump + 1) % 65
            set_cross(pump < 25)
        end
        local fish = probe.read_u32(FISH) or 0
        local flags, x, y, z, t22, rx, ry, rz, sc, clip = 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        if fish ~= 0 and probe.in_ram(fish) then
            flags = probe.read_u32(fish + 0x10) or 0
            x = s16(probe.read_u16(fish + 0x14) or 0)
            y = s16(probe.read_u16(fish + 0x16) or 0)
            z = s16(probe.read_u16(fish + 0x18) or 0)
            t22 = s16(probe.read_u16(fish + 0x22) or 0)
            rx = s16(probe.read_u16(fish + 0x24) or 0)
            ry = s16(probe.read_u16(fish + 0x26) or 0)
            rz = s16(probe.read_u16(fish + 0x28) or 0)
            sc = probe.read_u16(fish + 0x72) or 0
            clip = probe.read_u16(fish + 0x5C) or 0
        end
        csv:write(string.format("%d,0x%X,%d,%d,%d,%d,%d,%d,0x%08X,0x%08X,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d\n",
            el, st, hooked, tension, probe.read_u32(RECORD) or 0,
            probe.read_u32(SUB) or 0, probe.read_u32(SEAT) or 0,
            probe.read_u32(SPECIES) or 0, fish, flags, x, y, z, t22, rx, ry, rz, sc, clip,
            s16(probe.read_u16(LINE_XY) or 0), s16(probe.read_u16(LINE_XY + 2) or 0)))
        if catch_at ~= nil then
            local t = el - catch_at
            if t % EVERY == 0 then screenshot(string.format("shot_%05d", el)) end
            if SAVE_AT[t] then
                probe.sstate.save(probe.out_path(string.format("catch_t%d.rawsstate", t)))
            end
            if t >= AFTER then
                csv:close(); csv = nil
                PCSX.quit(0)
            end
        elseif el % 300 == 0 then
            screenshot(string.format("shot_%05d", el))
        end
    end,
})
