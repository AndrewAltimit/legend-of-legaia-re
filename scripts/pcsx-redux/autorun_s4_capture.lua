-- autorun_s4_capture.lua
--
-- Chain S4: from the s3_rimelm_freeroam anchor, wander Rim Elm until the FIRST
-- scene transition (a door / village exit), then checkpoint. Locomotion is
-- camera-remapped, so fixed directions don't explore well; instead this does a
-- bump-and-turn wander - hold a direction, and when the player position stops
-- changing (blocked by a wall), rotate to the next direction. Any dialogue that
-- a bumped NPC opens is dismissed by pulsing CROSS. A scene-name change (away
-- from town01) = a transition -> checkpoint S4.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_CKPT_LABEL, LEGAIA_SETTLE,
--      LEGAIA_MAX_FRAMES.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP   = 0x8001698C
local GM         = 0x8007B83C
local SCENE_NAME = 0x8007050C
local PLAYER     = 0x8007C364

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s4_cap")
local CKPT_LABEL = env.getenv("LEGAIA_CKPT_LABEL", "s4_transition")
local HOME_SCENE = env.getenv("LEGAIA_HOME_SCENE", "town01")
local SETTLE     = tonumber(env.getenv("LEGAIA_SETTLE", "20")) or 20
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "6000")) or 6000
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2
local START_SEED = tonumber(env.getenv("LEGAIA_SEED", "0")) or 0

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/cap.log", "w")
local function log(s) PCSX.log("[s4] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end
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
local function ppos()
    local pp = ru32(PLAYER); if pp == nil then return nil end
    return (ru32(pp+0x14) or 0), (ru32(pp+0x18) or 0)
end

local function write_checkpoint(label)
    local ok = pcall(function()
        local w = PCSX.createSaveState()
        local path = OUT_DIR .. "/" .. label .. ".rawsstate"
        local fh = Support.File.open(path, "CREATE"); fh:writeMoveSlice(w); fh:close()
        log("checkpoint written: " .. path)
    end)
    if not ok then log("checkpoint FAILED") end
    return ok
end

local vsync, loaded = 0, false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync = vsync + 1
    if not loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        loaded = true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
end)

-- bump-and-turn wander: hold DIRS[di]; when blocked, rotate di.
local DIRS = { "UP", "RIGHT", "DOWN", "LEFT" }
local di = (START_SEED % #DIRS) + 1
local held = nil
local last_x, last_y, stuck = nil, nil, 0
local frame = 0
local cross_until, cross_cd = 0, 0
local interact_tries, held_face = 0, nil
local phase = "WANDER"   -- WANDER -> SETTLE_NEW -> DONE
local new_scene, target_since, quit_at = nil, nil, nil

local function set_dir(d)
    if held and held ~= d then pad.release(pad.BTN[held]) end
    if d and held ~= d then pad.force(pad.BTN[d]) end
    held = d
end

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame = frame + 1
    if frame >= MAX_FRAMES then
        if held then pad.release(pad.BTN[held]) end
        log(string.format("MAX_FRAMES (%d) without transition; pos=%s", MAX_FRAMES, tostring(read_scene())))
        if LOG then LOG:close() end; PCSX.quit(0)
    end

    local sc = read_scene()
    -- transition?
    if phase == "WANDER" and sc ~= HOME_SCENE and sc ~= "" then
        if held then pad.release(pad.BTN[held]); held = nil end
        new_scene = sc
        log(string.format("*** [f%d] TRANSITION town01 -> %q (mode=0x%02X) ***", frame, sc, ru8(GM) or 0xFF))
        phase = "SETTLE_NEW"
        return
    end

    if phase == "WANDER" then
        -- dismiss any dialogue an NPC opened (engaged with no movement)
        if engaged() then
            if cross_until > 0 and frame >= cross_until then pad.release(pad.BTN.CROSS); cross_until = 0; cross_cd = frame + 8
            elseif cross_until == 0 and frame >= cross_cd then pad.force(pad.BTN.CROSS); cross_until = frame + 3 end
            return
        end
        -- walk + bump detection
        set_dir(DIRS[di])
        local x, y = ppos()
        if last_x ~= nil and x == last_x and y == last_y then stuck = stuck + 1 else stuck = 0 end
        last_x, last_y = x, y
        if stuck >= 18 then
            -- blocked: the wall ahead might be a house door - INTERACT (CROSS)
            -- while facing it before giving up and turning. A door's script
            -- triggers the scene change; an NPC opens dialogue (dismissed above).
            if interact_tries < 1 then
                set_dir(nil) -- stop walking so facing holds
                if cross_until == 0 and frame >= cross_cd then
                    pad.force(pad.BTN.CROSS); cross_until = frame + 3
                    log(string.format("[f%d] blocked - interact (CROSS) facing %s", frame, held_face or "?"))
                    interact_tries = interact_tries + 1
                end
            else
                di = (di % #DIRS) + 1; stuck = 0; interact_tries = 0
                log(string.format("[f%d] blocked -> turn to %s (scene=%q)", frame, DIRS[di], sc))
            end
        else
            interact_tries = 0; held_face = DIRS[di]
        end
        if (frame % 300) == 0 then log(string.format("[f%d] wander dir=%s scene=%q pos=(%d,%d)", frame, DIRS[di], sc, x or 0, y or 0)) end
        return
    end

    if phase == "SETTLE_NEW" then
        -- wait for the new scene to be a stable field-run state, then checkpoint
        local m = ru8(GM) or 0xFF
        if m == 0x03 and sc == new_scene then
            if target_since == nil then target_since = frame
            elseif frame - target_since >= SETTLE then
                log(string.format("[f%d] settled in %q (mode 0x03); checkpointing", frame, new_scene))
                write_checkpoint(CKPT_LABEL); phase = "DONE"; quit_at = frame + 2
            end
        else target_since = nil end
        return
    end

    if phase == "DONE" and quit_at and frame >= quit_at then
        if LOG then LOG:close() end; PCSX.quit(0)
    end
end)

log("s4 capture armed (bump-and-turn wander, seed=" .. START_SEED .. ")")
