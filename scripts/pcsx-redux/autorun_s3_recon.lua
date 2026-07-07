-- autorun_s3_recon.lua
--
-- S3 recon: resume the s2_rimelm_town01 anchor and OBSERVE the field/interaction
-- state per frame (no input), to design the S3 (first NPC dialogue) drive. Logs
-- the player engaged flag (0x80000 = cutscene/encounter/interaction owns the
-- player), the field-control interact flag + event counter, the player XZ
-- position, and game_mode/scene. Driven off an exec-bp on the field tick
-- FUN_8001698C so it works even if GPU::Vsync delivery to Lua is sparse.
--
-- Env: LEGAIA_SSTATE (resume), LEGAIA_OUT_DIR, LEGAIA_MAX_FRAMES.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local MASH  = env.getenv("LEGAIA_MASH", "") == "1"  -- "1" = mash to advance the opening
local SWEEP = env.getenv("LEGAIA_SWEEP", "") == "1" -- "1" = cycle input groups (diagnostic)

local GM         = 0x8007B83C
local SCENE_NAME = 0x8007050C
local PLAYER_PTR = 0x8007C364 -- -> player/camera-anchor actor
local SCENE_PTR  = 0x801C6EA4 -- -> current scene/field-control struct
local FIELD_BP   = 0x8001698C

local START_SAVE  = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR     = env.getenv("LEGAIA_OUT_DIR", "captures/s3_recon")
local MAX_FRAMES  = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "900")) or 900
local START_DELAY = tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/recon.log", "w")
local function log(s) PCSX.log("[s3] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end

local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end
local function s32(v)  if v == nil then return nil end; if v >= 0x80000000 then return v - 0x100000000 end; return v end

local function read_scene()
    local s = {}
    for i = 0, 7 do
        local b = ru8(SCENE_NAME + i) or 0
        if b < 0x20 or b >= 0x7f then break end
        s[#s+1] = string.char(b)
    end
    return table.concat(s)
end

local vsync, loaded = 0, false
local function on_vsync()
    vsync = vsync + 1
    if not loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        loaded = true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED load "..START_SAVE))
    end
end
-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)

local tick = 0
local mash_until = 0
local first_clear = nil
local last_eng = nil
local clear_ticks = 0
local last_phase = nil
local cur_btns = {}
local last_px, last_pz = nil, nil
bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    tick = tick + 1
    if MASH and not SWEEP then
        -- Sustained CROSS+CIRCLE mash (both confirm conventions) to advance the
        -- town01 opening dialogue toward free-roam.
        if mash_until > 0 and tick >= mash_until then
            pad.release(pad.BTN.CROSS); pad.release(pad.BTN.CIRCLE); mash_until = 0
        elseif (tick % 20) == 0 and mash_until == 0 then
            pad.force(pad.BTN.CROSS); pad.force(pad.BTN.CIRCLE); mash_until = tick + 6
        end
    end
    if MASH and SWEEP then
        -- Input SWEEP: cycle button groups to find what advances the town01
        -- opening (CROSS alone does not). Each phase lasts SWEEP_LEN ticks; pulse
        -- the phase's buttons every 20 ticks. Log which phase is active.
        local SWEEP_LEN = 200
        local phases = {
            { name = "CROSS",       btns = { pad.BTN.CROSS } },
            { name = "CIRCLE",      btns = { pad.BTN.CIRCLE } },
            { name = "START",       btns = { pad.BTN.START } },
            { name = "UP+CROSS",    btns = { pad.BTN.UP,    pad.BTN.CROSS } },
            { name = "DOWN+CROSS",  btns = { pad.BTN.DOWN,  pad.BTN.CROSS } },
            { name = "LEFT",        btns = { pad.BTN.LEFT } },
            { name = "RIGHT",       btns = { pad.BTN.RIGHT } },
            { name = "TRIANGLE",    btns = { pad.BTN.TRIANGLE } },
            { name = "SQUARE",      btns = { pad.BTN.SQUARE } },
        }
        local pi = (math.floor((tick - 1) / SWEEP_LEN) % #phases) + 1
        local ph = phases[pi]
        if pi ~= last_phase then
            log(string.format("== sweep phase %s (tick %d) ==", ph.name, tick)); last_phase = pi
        end
        if mash_until > 0 and tick >= mash_until then
            for _, b in ipairs(cur_btns) do pad.release(b) end
            mash_until = 0
        elseif (tick % 20) == 0 and mash_until == 0 then
            cur_btns = ph.btns
            for _, b in ipairs(cur_btns) do pad.force(b) end
            mash_until = tick + 6
        end
    end
    -- log every engaged-flag (0x80000) transition + interact-flag transition, so
    -- the box-by-box opening cadence + any sustained free-roam window is visible.
    do
        local pp = ru32(PLAYER_PTR)
        local sp = ru32(SCENE_PTR)
        local fl = pp and ru32(pp + 0x10) or nil
        local eng = fl ~= nil and (math.floor(fl / 0x80000) % 2 == 1) or nil
        local itx = sp and ru8(sp + 0x60) or nil
        if eng ~= nil and eng ~= last_eng then
            log(string.format(">> tick %d: eng80000 %s->%s flags=0x%08X interact=%s",
                tick, tostring(last_eng), tostring(eng), fl, tostring(itx)))
            if not eng and first_clear == nil then first_clear = tick end
            last_eng = eng
        end
        if eng == false then clear_ticks = clear_ticks + 1 end
        -- log player-position changes (which sweep phase actually moves the lead)
        local px = pp and s32(ru32(pp + 0x14)) or nil
        local pz = pp and s32(ru32(pp + 0x18)) or nil
        if px ~= nil and (px ~= last_px or pz ~= last_pz) then
            if last_px ~= nil then
                log(string.format("~~ tick %d: pos moved (%s,%s)->(%s,%s)", tick,
                    tostring(last_px), tostring(last_pz), tostring(px), tostring(pz)))
            end
            last_px, last_pz = px, pz
        end
    end
    if (tick % 120) == 0 then
        local pp = ru32(PLAYER_PTR)
        local sp = ru32(SCENE_PTR)
        -- gate-type introspection at the stall: field-control dialog byte
        -- (sp+0x62), option-picker cursor (sp+0xc), event counter (sp+0xA),
        -- and the dialog pager lines-per-box global (0x801F2740).
        local dlg_byte = sp and ru8(sp + 0x62) or nil
        local cursor   = sp and ru8(sp + 0x0C) or nil
        local pager    = ru8(0x801F2740)
        log(string.format("   [gate] dlg62=%s cursor=%s pager=%s",
            dlg_byte ~= nil and string.format("0x%02X", dlg_byte) or "nil",
            cursor ~= nil and string.format("0x%02X", cursor) or "nil",
            pager ~= nil and string.format("0x%02X", pager) or "nil"))
        local flags = pp and ru32(pp + 0x10) or nil
        local px = pp and s32(ru32(pp + 0x14)) or nil
        local pz = pp and s32(ru32(pp + 0x18)) or nil
        local interact = sp and ru8(sp + 0x60) or nil
        local evtcnt   = sp and ru8(sp + 0x0A) or nil
        local engaged = flags and (flags % 0x100000 >= 0x80000 and (math.floor(flags / 0x80000) % 2 == 1)) or false
        log(string.format("t=%-5d mode=0x%02X scene=%-7q player=%s flags=%s eng80000=%s interact=%s evt=%s xz=(%s,%s)",
            tick, ru8(GM) or 0xFF, read_scene(),
            pp and string.format("0x%08X", pp) or "nil",
            flags and string.format("0x%08X", flags) or "nil",
            tostring(engaged),
            interact ~= nil and string.format("%d", interact) or "nil",
            evtcnt ~= nil and string.format("%d", evtcnt) or "nil",
            tostring(px), tostring(pz)))
    end
    if tick >= MAX_FRAMES then
        log(string.format("SUMMARY: ran %d ticks, first_clear=%s, total clear ticks=%d",
            tick, tostring(first_clear), clear_ticks))
        if LOG then LOG:close() end
        PCSX.quit(0)
    end
end)

log("s3 recon armed; resume + observe field state")
