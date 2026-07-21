-- autorun_s2s3_atlas_stamp.lua
--
-- FINDING (thread-closing): at v2510 of the replay the probe catches
--   MoveImage src=(852,336,6,16) dst=(852,268)   ra=0x801E1B8C
--   MoveImage src=(852,368,4,8)  dst=(853,284)   ra=0x801E1B8C
-- once - the Noa face-cell stamp from the field-VM `4C 60` arm inside
-- FUN_801DE840 (jal at 0x801E1B84), i.e. town01 MAN P2[3] `OP` (the Rim
-- Elm opening timeline record) body offsets +0x392/+0x3A0. The parked
-- frame at (852,336) differs from the boot cell at exactly strip row 15
-- cols 1/4/5 = the three F-variant halfwords at VRAM rows 271; the
-- parked end state reproduces the s3 anchor's band byte-exact. The Vahn
-- blink pair (837|832,328,5,20)->(832,264) + (832,368,3,12)->(832,300)
-- fires repeatedly from the same arm throughout the opening. See
-- docs/formats/character-mesh.md, runtime scroll-cell residue.
--
-- Successor to autorun_s2s3_scroll_installer.lua after its clean negative:
-- the s2 -> s3 replay executes NO move-VM scroll install (op 0x1E / 0x45
-- bodies never run) and no dispatch-4 tick, yet the s3 anchor's VRAM shows
-- the F-variant. Offline full-strip diff of the s3 state resolves the
-- residue's true shape: a ONE-ROW band copy (x in [852..858], y 273) ->
-- (x, 271), width 5..7 - a MoveImage frame stamp, not a parked wrap-scroll
-- phase.
--
-- This probe traps the three libgpu image wrappers during the same replay:
--   0x8005842C  StoreImage-style (rect, buf)   [VRAM -> RAM]
--   0x80058490  MoveImage-style  (rect, dx, dy)[VRAM -> VRAM]
--   0x800583C8  LoadImage-style  (rect, buf)   [RAM -> VRAM]
-- logging every call whose rect (or dest) touches x in [832,880) with
-- y in [256,384) - the 0874 s2 character-atlas band - plus the caller ra.
-- The name-entry walk is replayed with the autorun_s3_capture.lua phase
-- machine; at the end a raw savestate is parked for offline VRAM diffing.
--
--   timeout --kill-after=60s 3000s \
--       bash scripts/pcsx-redux/run_probe.sh \
--       --scenario s2_rimelm_town01 \
--       --lua scripts/pcsx-redux/autorun_s2s3_atlas_stamp.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP   = 0x8001698C
local CURSOR     = 0x8007BB88
local PLAYER     = 0x8007C364
local SCENE_NAME = 0x8007050C

local STORE_FN = 0x8005842C
local MOVE_FN  = 0x80058490
local LOAD_FN  = 0x800583C8

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s2s3_stamp")
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "4200")) or 4200
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/stamp.log", "w")
local function log(s) PCSX.log("[stamp] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end

local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru16(a) return mem.in_ram(a) and mem.read_u16(a) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end
local function reg(r, name)
    local v = tonumber(r.GPR.n[name]) or 0
    if v < 0 then v = v + 0x100000000 end
    return v % 0x100000000
end
local function read_scene()
    local s = {}
    for i = 0, 7 do local b = ru8(SCENE_NAME+i) or 0; if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end
local function engaged()
    local pp = ru32(PLAYER); if pp == nil then return nil end
    local fl = ru32(pp+0x10); if fl == nil then return nil end
    return math.floor(fl/0x80000) % 2 == 1
end

local vsync, loaded = 0, false
local atlas_hits, other_calls = 0, 0

local function in_atlas(x, y, w, h)
    if x == nil then return false end
    local x1, y1 = x + (w or 0), y + (h or 0)
    return x1 > 832 and x < 880 and y1 > 256 and y < 384
end

local function img_call(kind)
    local r = PCSX.getRegisters()
    local rp = reg(r, "a0")
    local x = ru16(rp); local y = ru16(rp + 2)
    local w = ru16(rp + 4); local h = ru16(rp + 6)
    local ra = reg(r, "ra")
    local a1 = reg(r, "a1")
    local a2 = reg(r, "a2")
    local hit = in_atlas(x, y, w, h)
    if kind == "Move" then
        -- dest (a1, a2) can hit the atlas even when src doesn't
        local dx = a1 % 0x10000; local dy = a2 % 0x10000
        if dx >= 0x8000 then dx = dx - 0x10000 end
        if dy >= 0x8000 then dy = dy - 0x10000 end
        hit = hit or in_atlas(dx, dy, w, h)
    end
    if not hit then other_calls = other_calls + 1; return end
    atlas_hits = atlas_hits + 1
    if atlas_hits > 200 then return end
    if kind == "Move" then
        log(string.format("[v%d] MoveImage src=(%d,%d,%d,%d) dst=(%d,%d) ra=0x%08X",
            vsync, x or -1, y or -1, w or -1, h or -1, a1 % 0x10000, a2 % 0x10000, ra))
    else
        log(string.format("[v%d] %sImage rect=(%d,%d,%d,%d) buf=0x%08X ra=0x%08X",
            vsync, kind, x or -1, y or -1, w or -1, h or -1, a1, ra))
    end
end

-- StoreImage (0x8005842C) reads VRAM -> RAM so it cannot write the band;
-- keeping the armed set minimal after a SIGSEGV with 6 BPs armed. The
-- op-0x1E / op-0x45 install bodies were already proven silent over this
-- replay by autorun_s2s3_scroll_installer.lua.
bp.arm(MOVE_FN,  "Exec", 4, "move_image",  function() img_call("Move")  end)
bp.arm(LOAD_FN,  "Exec", 4, "load_image",  function() img_call("Load")  end)

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync = vsync + 1
    if not loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        loaded = true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
end)

local function park_state(name)
    local ok = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(OUT_DIR .. "/" .. name, "CREATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    log(ok and ("parked " .. name) or ("park FAILED " .. name))
end

local pulse_btns, pulse_until = {}, 0
local function pulse(btns, frame, dur)
    for _, b in ipairs(btns) do pad.force(pad.BTN[b]) end
    pulse_btns = btns; pulse_until = frame + (dur or 3)
end

local phase = "SETTLE"
local frame = 0
local cooldown = 0
local freeroam_since = nil

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame = frame + 1
    if pulse_until > 0 and frame >= pulse_until then
        for _, b in ipairs(pulse_btns) do pad.release(pad.BTN[b]) end
        pulse_until = 0; cooldown = frame + 10
    end
    if frame >= MAX_FRAMES then
        log(string.format("MAX_FRAMES (phase=%s atlas_hits=%d)", phase, atlas_hits))
        park_state("end_state.rawsstate")
        if LOG then LOG:close() end; PCSX.quit(0)
    end

    local cur = ru16(CURSOR)
    local eng = engaged()
    if (frame % 240) == 0 then
        log(string.format("[f%d/v%d] phase=%s cursor=%s eng=%s scene=%q atlas=%d other=%d",
            frame, vsync, phase, cur and string.format("0x%04X", cur) or "nil",
            tostring(eng), read_scene(), atlas_hits, other_calls))
    end

    if pulse_until > 0 or frame < cooldown then return end

    if phase == "SETTLE" then
        if frame >= 760 and eng and cur ~= nil then phase = "SELECT_END"; log("phase -> SELECT_END") end
        return
    end
    if phase == "SELECT_END" then
        if cur == nil then return end
        if cur >= 116 and cur <= 118 then
            pulse({"CROSS"}, frame, 3); phase = "CONFIRM"; log("selected End (CROSS) -> CONFIRM")
        else
            local row, col = math.floor(cur/17), cur%17
            if row < 6 then pulse({"DOWN"}, frame, 3)
            elseif col < 14 then pulse({"RIGHT"}, frame, 3)
            else pulse({"LEFT"}, frame, 3) end
        end
        return
    end
    if phase == "CONFIRM" then
        if eng == false then
            phase = "FREEROAM"; log(string.format("[f%d] engaged cleared -> FREEROAM", frame)); return
        end
        mem.write_u8(0x8007B458, 0)
        pulse({"CROSS"}, frame, 3)
        return
    end
    if phase == "FREEROAM" then
        if freeroam_since == nil then freeroam_since = frame end
        if frame - freeroam_since >= 300 then
            log(string.format("[f%d] done (atlas_hits=%d)", frame, atlas_hits))
            park_state("end_state.rawsstate")
            if LOG then LOG:close() end; PCSX.quit(0)
        end
        return
    end
end)

log("s2s3 atlas-stamp probe armed")
