-- autorun_s3_capture.lua
--
-- Complete the town01 name-entry screen and capture the S3 free-roam anchor.
-- The cursor starts on "End" (idx 116) with "Vahn" pre-filled; CROSS = confirm
-- (mask_sel 0x44 at 0x800846D0). Sequence: select End (CROSS) -> drive the Yes/No
-- sub-state to "Yes" -> name entry completes, STATE_RESUME finishes, the player
-- engaged flag (0x80000) clears -> free-roam -> checkpoint.
--
-- The name-entry effect-actor's sub-sub-state lives at actor+0x54 (0 init /
-- 1 interactive / 2-4 confirm); we capture the actor pointer off FUN_801F159C
-- and log it so the Yes/No drive is observable. Confirm is adaptive: it cycles
-- CROSS, UP+CROSS, DOWN+CROSS until the player frees up.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_CKPT_LABEL, LEGAIA_SETTLE.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP   = 0x8001698C
local SR_HANDLER = 0x801F159C
local CURSOR     = 0x8007BB88
local PLAYER     = 0x8007C364
local SCENE_PTR  = 0x801C6EA4
local SR_STATE   = 0x8007B450
local SCENE_NAME = 0x8007050C

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s3_cap")
local CKPT_LABEL = env.getenv("LEGAIA_CKPT_LABEL", "s3_freeroam")
local SETTLE     = tonumber(env.getenv("LEGAIA_SETTLE", "30")) or 30
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "4000")) or 4000
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/cap.log", "w")
local function log(s) PCSX.log("[cap] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru16(a) return mem.in_ram(a) and ((ru8(a) or 0)+0x100*(ru8(a+1) or 0)) or nil end
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

local vsync, loaded = 0, false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync = vsync + 1
    if not loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        loaded = true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
end)

-- checkpoint writer (raw createSaveState slice; host gzips + catalogs)
local function write_checkpoint()
    local ok = pcall(function()
        local w = PCSX.createSaveState()
        local path = OUT_DIR .. "/" .. CKPT_LABEL .. ".rawsstate"
        local fh = Support.File.open(path, "CREATE")
        fh:writeMoveSlice(w); fh:close()
        log("checkpoint written: " .. path)
    end)
    if not ok then log("checkpoint FAILED") end
    return ok
end

local g_actor = nil
bp.arm(SR_HANDLER, "Exec", 4, "sr_handler", function()
    local r = PCSX.getRegisters()
    local a0 = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
    if a0 < 0 then a0 = a0 + 0x100000000 end
    g_actor = a0
end)

-- clean button pulse helper
local pulse_btns, pulse_until = {}, 0
local function pulse(btns, frame, dur)
    for _, b in ipairs(btns) do pad.force(pad.BTN[b]) end
    pulse_btns = btns; pulse_until = frame + (dur or 3)
end

local phase = "SETTLE"
local frame = 0
local target_since, quit_at = nil, nil
local cooldown = 0
local confirm_step = 0
local last_sub = -1
local last_osub = -1

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame = frame + 1
    -- release any active pulse
    if pulse_until > 0 and frame >= pulse_until then
        for _, b in ipairs(pulse_btns) do pad.release(pad.BTN[b]) end
        pulse_until = 0; cooldown = frame + 10
    end
    if frame >= MAX_FRAMES then log("MAX_FRAMES reached without capture"); if LOG then LOG:close() end; PCSX.quit(0) end

    local cur = ru16(CURSOR)
    local eng = engaged()
    local sub = g_actor and ru8(g_actor + 0x54) or nil
    local osub = g_actor and ru16(g_actor + 0x50) or nil
    if osub ~= nil and osub ~= last_osub then
        log(string.format("[f%d] actor+0x50 (OUTER): %s -> 0x%02X", frame,
            last_osub < 0 and "?" or string.format("0x%02X", last_osub), osub))
        last_osub = osub
    end
    if sub ~= nil and sub ~= last_sub then
        log(string.format("[f%d] actor+0x54: %s -> 0x%02X (cursor=%s eng=%s scene=%q)",
            frame, last_sub < 0 and "?" or string.format("0x%02X", last_sub), sub,
            cur and string.format("0x%04X", cur) or "nil", tostring(eng), read_scene()))
        last_sub = sub
    end
    if (frame % 120) == 0 then
        log(string.format("[f%d] phase=%s cursor=%s eng=%s sub=%s sr=0x%08X scene=%q",
            frame, phase, cur and string.format("0x%04X", cur) or "nil", tostring(eng),
            sub ~= nil and string.format("0x%02X", sub) or "nil", ru32(SR_STATE) or 0, read_scene()))
    end

    if pulse_until > 0 or frame < cooldown then return end

    if phase == "SETTLE" then
        if frame >= 760 and eng and cur ~= nil then phase = "SELECT_END"; log("phase -> SELECT_END") end
        return
    end

    if phase == "SELECT_END" then
        -- ensure cursor on an End cell (116/117/118); navigate if not
        if cur == nil then return end
        if cur >= 116 and cur <= 118 then
            pulse({"CROSS"}, frame, 3); phase = "CONFIRM"; log("selected End (CROSS) -> CONFIRM")
        else
            -- move toward End (row 6, col >=14): DOWN to row 6, then RIGHT
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
        -- The End -> Yes/No confirm: actor+0x54 = 2/4 is the prompt. Re-reading
        -- the sub-4 handler: the toggle _DAT_8007B458 default 1 takes the branch
        -- that LOOPS (actor+0x50 stays 0x22); the toggle = 0 branch ADVANCES the
        -- OUTER state actor+0x50 to 0x1A (out of name entry). So hold the toggle
        -- at 0 and pulse CROSS to commit the advancing option.
        mem.write_u8(0x8007B458, 0)
        pulse({"CROSS"}, frame, 3)
        return
    end

    if phase == "FREEROAM" then
        if eng == false and read_scene() == "town01" then
            if target_since == nil then target_since = frame
            elseif frame - target_since >= SETTLE then
                log(string.format("[f%d] free-roam settled (scene=town01); checkpointing", frame))
                write_checkpoint(); phase = "DONE"; quit_at = frame + 2
            end
        else target_since = nil end
        return
    end

    if phase == "DONE" and quit_at and frame >= quit_at then
        if LOG then LOG:close() end; PCSX.quit(0)
    end
end)

log("s3 capture armed")
