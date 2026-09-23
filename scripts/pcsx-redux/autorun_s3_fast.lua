-- autorun_s3_fast.lua
--
-- Recompiler-speed S3: resume the S2 checkpoint (town01 arrival), let the
-- opening run into the name-entry screen, accept the default name, and
-- checkpoint first free-roam. The vsync-only sibling of
-- autorun_s3_capture.lua, whose field-tick breakpoint needs the interpreter
-- + debugger core; the input sequence is the same:
--   * wait for the player-engaged flag (*0x8007C364 +0x10 & 0x80000) and the
--     name-entry cursor (0x8007BB88) on an End cell (116..118) - the cursor
--     starts there with "Vahn" pre-filled;
--   * CROSS selects End, opening "Is this name okay?" with the cursor on No;
--   * UP moves it to Yes and CROSS commits - pad input only, and only while
--     the prompt is open. The name-entry actor (callback word +0x0C =
--     FUN_801F159C) carries its sub-state at +0x54: 1 = the glyph grid,
--     2..4 = the confirm. A press timed blind lands on the grid instead - an
--     UP moves the grid cursor off End and the next CROSS types a glyph into
--     the name, which then rides every later frame's HUD. The prompt's
--     cursor is _DAT_8007B458 (1 = No, 0 = Yes). (The interpreter driver
--     writes 0 there each tick; on this path the write does not take.)
--   * CROSS advances the opening dialogue that follows the confirm;
--   * the engaged flag clears -> free-roam -> settle -> checkpoint.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_CKPT_LABEL (s3_freeroam),
--      LEGAIA_SETTLE (60 vsyncs), LEGAIA_MIN_VSYNC (1500: earliest End press),
--      LEGAIA_MAX_VSYNC (12000).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")

local PLAYER, CURSOR, TOGGLE, SCENE_NAME = 0x8007C364, 0x8007BB88, 0x8007B458, 0x8007050C
local OUT_DIR = env.getenv("LEGAIA_OUT_DIR", "captures/s3_fast")
local LABEL   = env.getenv("LEGAIA_CKPT_LABEL", "s3_freeroam")
local START   = env.getenv("LEGAIA_SSTATE", "")
local SETTLE  = tonumber(env.getenv("LEGAIA_SETTLE", "60")) or 60
local MIN_V   = tonumber(env.getenv("LEGAIA_MIN_VSYNC", "1500")) or 1500
local MAX_V   = tonumber(env.getenv("LEGAIA_MAX_VSYNC", "12000")) or 12000
local DEBUG   = env.getenv("LEGAIA_DEBUG_CKPT", "") == "1"

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
        local fh = Support.File.open(path, "CREATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    log(string.format("checkpoint %s ok=%s %s", path, tostring(ok), tostring(err or "")))
end

local v, loaded, phase, since, pulse_end, seen_engaged = 0, false, "WAIT", nil, 0, false
local pulse_btn = pad.BTN.CROSS
local function pulse(b, n) pad.force(b); pulse_btn = b; pulse_end = v + (n or 4) end
local function on_vsync()
    v = v + 1
    if not loaded then
        if v >= 2 then loaded = true; log((sstate.load(START) and "resumed " or "FAILED ") .. START) end
        return
    end
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
        if eng == false and scene() == "town01" then
            since = since or v
            if v - since >= SETTLE then
                local p = mem.read_u32(PLAYER)
                log(string.format("vsync %d: free-roam settled at (%d,%d); checkpointing", v,
                    mem.read_u16(p + 0x14) or -1, mem.read_u16(p + 0x18) or -1))
                checkpoint(); PCSX.quit(0)
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
