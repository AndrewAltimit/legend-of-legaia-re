-- autorun_player_clip_beat.lua
--
-- Per-vsync capture of the PLAYER actor's clip words across the town01
-- post-naming beat: resume S2 (town01 arrival), let the opening reach the
-- name entry, accept the default name exactly as autorun_s3_fast.lua does,
-- and log every vsync until free roam:
--   +0x5C requested clip, +0x5E bound clip, +0x62 control word (0x08 clamp,
--   0x100 end latch), +0x68 cursor (1/16 frame), +0x6A rate, the bound
--   clip's frame count (*(+0x4C)+2), and the clip base _DAT_8007BDD8.
-- The question: after `A2 F8 31` and `AC F8 03` (clamp cleared) does scene
-- record 48 loop through the `4A 3C` wait, or play once?
--
--   timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh \
--       --fast --lua scripts/pcsx-redux/autorun_player_clip_beat.lua \
--       --scenario s2_rimelm_town01 --iso <scratch copy>
--
-- The cave01 wake-up (Noa beside Terra) has no retail state of its own:
-- LEGAIA_CAVE01_REENTRY=1 resumes cave01_attached_light (a cave01 field run),
-- raises system flag 0x156 (byte 0x80085782 bit 0x02, MSB-first like flag 2 =
-- 0x80085758 bit 0x20) and writes MAIN INIT (2) to the mode word 0x8007B83C,
-- so FUN_801D6704 reloads cave01 and its entry script takes the 0x156 arm -
-- the same arm the story reaches. LEGAIA_FREE_SCENE names the scene whose
-- free roam ends the capture (default town01).
--
-- Output: <out>/player_clip.csv (plus s3.log). Integers only.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")

local PLAYER, CURSOR, TOGGLE, SCENE_NAME = 0x8007C364, 0x8007BB88, 0x8007B458, 0x8007050C
local OUT_DIR = env.getenv("LEGAIA_OUT_DIR", "captures/player_clip_beat")
local LABEL   = env.getenv("LEGAIA_CKPT_LABEL", "s3_freeroam")
local START   = env.getenv("LEGAIA_SSTATE", "")
local SETTLE  = tonumber(env.getenv("LEGAIA_SETTLE", "60")) or 60
local MIN_V   = tonumber(env.getenv("LEGAIA_MIN_VSYNC", "1500")) or 1500
local MAX_V   = tonumber(env.getenv("LEGAIA_MAX_VSYNC", "12000")) or 12000
local DEBUG   = env.getenv("LEGAIA_DEBUG_CKPT", "") == "1"
local REENTRY = env.getenv("LEGAIA_CAVE01_REENTRY", "") == "1"
local FREE_SCENE = env.getenv("LEGAIA_FREE_SCENE", "town01")

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/s3.log", "w")
local function log(s) PCSX.log("[s3f] " .. s); if LOG then LOG:write(s .. "\n"); LOG:flush() end end
local function scene()
    local t = {}
    for i = 0, 7 do
        local b = mem.read_u8(SCENE_NAME + i) or 0
        if b < 0x20 or b >= 0x7f then break end
        t[#t + 1] = string.char(b)
    end
    return table.concat(t)
end
local function engaged()
    local p = mem.read_u32(PLAYER)
    if not p or not mem.in_ram(p + 0x10) then return nil end
    return math.floor((mem.read_u32(p + 0x10) or 0) / 0x80000) % 2 == 1
end
local NAME_ENTRY_TICK = 0x801F159C
local function name_entry_actor()
    for h = 0x8007C34C, 0x8007C360, 4 do
        local a, n = mem.read_u32(h), 0
        while a and a ~= 0 and mem.in_ram(a) and n < 400 do
            if mem.read_u32(a + 0x0C) == NAME_ENTRY_TICK then return a end
            a = mem.read_u32(a); n = n + 1
        end
    end
    return nil
end
local function checkpoint(tag)
    local path = OUT_DIR .. "/" .. (tag or LABEL) .. ".rawsstate"
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(path, "TRUNCATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    log(string.format("checkpoint %s ok=%s %s", path, tostring(ok), tostring(err or "")))
end

local CLIP = io.open(OUT_DIR .. "/player_clip.csv", "w")
CLIP:write("vsync,phase,scene,eng,c5c,c5e,c62,c68,c6a,frames,base\n")
local function log_clip(v, phase)
    local p = mem.read_u32(PLAYER)
    if not p or not mem.in_ram(p + 0x68) then return end
    local clip = mem.read_u32(p + 0x4C) or 0
    local frames = (clip ~= 0 and mem.in_ram(clip + 2)) and (mem.read_u16(clip + 2) or -1) or -1
    CLIP:write(string.format("%d,%s,%s,%s,%d,%d,%d,%d,%d,%d,%d\n", v, phase, scene(), tostring(engaged()),
        mem.read_u16(p + 0x5C) or -1, mem.read_u16(p + 0x5E) or -1, mem.read_u16(p + 0x62) or -1,
        mem.read_u16(p + 0x68) or -1, mem.read_u16(p + 0x6A) or -1, frames,
        mem.read_u32(0x8007BDD8) or -1))
end
local v, loaded, phase, since, pulse_end, seen_engaged = 0, false, "WAIT", nil, 0, false
local pulse_btn = pad.BTN.CROSS
local function pulse(b, n) pad.force(b); pulse_btn = b; pulse_end = v + (n or 4) end
local function on_vsync()
    v = v + 1
    if not loaded then
        if v >= 2 then
            loaded = true; log((sstate.load(START) and "resumed " or "FAILED ") .. START)
            if REENTRY then
                mem.write_u8(0x80085782, bit.bor(mem.read_u8(0x80085782) or 0, 0x02))
                mem.write_u16(0x8007B83C, 2)
                log("cave01 re-entry: flag 0x156 raised, mode word = 2")
            end
        end
        return
    end
    log_clip(v, phase)
    if pulse_end > 0 and v >= pulse_end then pad.release(pulse_btn); pulse_end = 0 end
    local eng = engaged()
    local cur = mem.read_u16(CURSOR)
    if eng then seen_engaged = true end
    if v % 300 == 0 then
        log(string.format("vsync %d phase=%s eng=%s cursor=%s scene=%s toggle=%s", v, phase, tostring(eng), tostring(cur), scene(), tostring(mem.read_u8(TOGGLE))))
        if DEBUG and phase ~= "FREE" then checkpoint(string.format("dbg_%05d", v)) end
    end
    if phase == "WAIT" then
        if v >= MIN_V and eng and cur and cur >= 116 and cur <= 118 then
            pulse(pad.BTN.CROSS)
            phase = "CONFIRM"; log(string.format("vsync %d: End selected (cursor %d)", v, cur))
        end
    elseif phase == "CONFIRM" then
        local ne = name_entry_actor()
        local sub = ne and mem.read_u8(ne + 0x54)
        if eng == false then
            phase = "FREE"; log(string.format("vsync %d: engaged cleared", v))
        elseif pulse_end == 0 and v % 20 == 0 and sub then
            local yesno = mem.read_u8(TOGGLE)
            if sub >= 2 and sub <= 4 then
                if yesno ~= 0 then
                    pulse(pad.BTN.UP); log(string.format("vsync %d: prompt (sub %d), UP to Yes", v, sub))
                else
                    pulse(pad.BTN.CROSS); log(string.format("vsync %d: prompt (sub %d) on Yes, CROSS", v, sub))
                end
            elseif sub == 1 then
                if cur and cur >= 116 and cur <= 118 then
                    pulse(pad.BTN.CROSS); log(string.format("vsync %d: grid on End (%d), CROSS", v, cur))
                end
            else
                -- past the confirm: the opening's dialogue resumes
                pulse(pad.BTN.CROSS)
            end
        elseif pulse_end == 0 and v % 20 == 0 and not sub then
            pulse(pad.BTN.CROSS)
        end
    elseif phase == "FREE" then
        if eng == false and scene() == FREE_SCENE then
            since = since or v
            if v - since >= SETTLE then
                local p = mem.read_u32(PLAYER)
                log(string.format("vsync %d: free-roam settled at (%d,%d); checkpointing", v,
                    mem.read_u16(p + 0x14) or -1, mem.read_u16(p + 0x18) or -1))
                CLIP:close(); PCSX.quit(0)
            end
        else
            since = nil
        end
    end
    if v >= MAX_V then log("max vsync; quitting"); PCSX.quit(1) end
end

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
log("armed")
