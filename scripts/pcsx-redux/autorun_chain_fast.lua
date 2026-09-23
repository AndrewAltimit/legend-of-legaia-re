-- autorun_chain_fast.lua
--
-- Recompiler-speed segment driver: resume a checkpoint (or cold boot), mash the
-- advance button every few vsyncs, and checkpoint when a target scene / mode /
-- actor flag is reached. The vsync-only sibling of autorun_play_from_boot.lua:
-- that driver ticks from two exec breakpoints, which need the interpreter +
-- debugger core, and on a loaded box that core runs the opening at a few
-- frames per second. Under `--fast` (dynarec) GPU::Vsync is delivered through
-- the title and prologue XA streams (the property autorun_boot_continue.lua
-- relies on), so a vsync listener can drive the whole chain.
--
-- Run:
--   LEGAIA_SSTATE=<ckpt.sstate> LEGAIA_CKPT_SCENE=town01 \
--     bash scripts/pcsx-redux/run_probe.sh --fast --lua scripts/pcsx-redux/autorun_chain_fast.lua
--   cold boot: LEGAIA_NO_SSTATE=1 LEGAIA_FASTBOOT=1 (START+CROSS mash while the
--   game mode is not a field mode, so the title's PRESS START + NEW GAME pass).
--
-- Env (all optional):
--   LEGAIA_SSTATE / LEGAIA_NO_SSTATE   resume save / cold boot
--   LEGAIA_CKPT_SCENE    target scene name (e.g. town01)
--   LEGAIA_CKPT_MODE     target game mode (default 3) - used with or without SCENE
--   LEGAIA_CKPT_PTR/OFF/MASK/WANT  actor-flag target (as autorun_play_from_boot)
--   LEGAIA_CKPT_LABEL    checkpoint stem (default chain)
--   LEGAIA_SETTLE        vsyncs at the target before checkpointing (default 40)
--   LEGAIA_MASH_EVERY    vsyncs between presses (default 20)
--   LEGAIA_MASH_BTN      field-mode mash buttons, "+"-joined (default CROSS)
--   LEGAIA_NO_MASH       1 = press nothing (settle-and-checkpoint only)
--   LEGAIA_HOLD_BTN      a direction held for the whole run (e.g. DOWN), released
--                        once the target is reached
--   LEGAIA_CKPT_WARP     1 = the target is an intra-scene warp: the player's
--                        position jumps more than 300 units between two vsyncs
--                        (a walk-touch door); settle counts from the jump
--   LEGAIA_MAX_VSYNC     safety cap (default 40000)
--   LEGAIA_OUT_DIR       output dir
-- Output: <OUT_DIR>/chain.log, <OUT_DIR>/<LABEL>.rawsstate (host-gzip it).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")

local GM, SCENE_NAME = 0x8007B83C, 0x8007050C
local OUT_DIR   = env.getenv("LEGAIA_OUT_DIR", "captures/chain_fast")
local LABEL     = env.getenv("LEGAIA_CKPT_LABEL", "chain")
local SCENE_T   = env.getenv("LEGAIA_CKPT_SCENE", "")
local MODE_T    = tonumber(env.getenv("LEGAIA_CKPT_MODE", "3")) or 3
local PTR       = tonumber(env.getenv("LEGAIA_CKPT_PTR", "0")) or 0
local OFF       = tonumber(env.getenv("LEGAIA_CKPT_OFF", "0")) or 0
local MASK      = tonumber(env.getenv("LEGAIA_CKPT_MASK", "0")) or 0
local WANT      = env.getenv("LEGAIA_CKPT_WANT", "clear")
local SETTLE    = tonumber(env.getenv("LEGAIA_SETTLE", "40")) or 40
local EVERY     = tonumber(env.getenv("LEGAIA_MASH_EVERY", "20")) or 20
local NO_MASH   = env.getenv("LEGAIA_NO_MASH", "") == "1"
local MAX_V     = tonumber(env.getenv("LEGAIA_MAX_VSYNC", "40000")) or 40000
local START     = env.getenv("LEGAIA_SSTATE", "")
local COLD      = env.getenv("LEGAIA_NO_SSTATE", "") == "1" or START == ""
local HOLD = env.getenv("LEGAIA_HOLD_BTN", "")
local WARP = env.getenv("LEGAIA_CKPT_WARP", "") == "1"
local FIELD_BTNS = {}
for b in string.gmatch(env.getenv("LEGAIA_MASH_BTN", "CROSS"), "[^+]+") do
    FIELD_BTNS[#FIELD_BTNS + 1] = pad.BTN[b]
end

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/chain.log", "w")
local function log(s) PCSX.log("[chain] " .. s); if LOG then LOG:write(s .. "\n"); LOG:flush() end end
local function scene()
    local t = {}
    for i = 0, 7 do
        local b = mem.read_u8(SCENE_NAME + i) or 0
        if b < 0x20 or b >= 0x7f then break end
        t[#t + 1] = string.char(b)
    end
    return table.concat(t)
end
local v = 0
local warped, last_xz = false, nil
local function player_xz()
    local p = mem.read_u32(0x8007C364)
    if not p or not mem.in_ram(p + 0x18) then return nil end
    local function s16(a) local v = mem.read_u16(a) or 0; if v >= 0x8000 then v = v - 0x10000 end; return v end
    return s16(p + 0x14), s16(p + 0x18)
end
local function at_target()
    if WARP then
        local x, z = player_xz()
        if x and last_xz and not warped and v > 30
            and (last_xz[1] ~= 0 or last_xz[2] ~= 0)
            and math.abs(x - last_xz[1]) + math.abs(z - last_xz[2]) > 300 then
            warped = true
            log(string.format("warp (%d,%d) -> (%d,%d)", last_xz[1], last_xz[2], x, z))
        end
        if x then last_xz = { x, z } end
        return warped and (mem.read_u8(GM) or 255) == MODE_T
    end
    if PTR ~= 0 then
        if SCENE_T ~= "" and scene() ~= SCENE_T then return false end
        local base = mem.read_u32(PTR)
        if base == nil or not mem.in_ram(base + OFF) then return false end
        local v = mem.read_u32(base + OFF) or 0
        local set = (math.floor(v / MASK) % 2) == 1
        if WANT == "set" then return set else return not set end
    end
    if SCENE_T ~= "" and scene() ~= SCENE_T then return false end
    return (mem.read_u8(GM) or 255) == MODE_T
end
local function checkpoint()
    local path = OUT_DIR .. "/" .. LABEL .. ".rawsstate"
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(path, "CREATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    log(string.format("checkpoint %s ok=%s %s", path, tostring(ok), tostring(err or "")))
end

-- Field modes take the field mash; anything else (title, menus, INIT) takes
-- START+CROSS, which is what the title's PRESS START + NEW GAME row needs.
local FIELDISH = { [0x03] = true, [0x15] = true }
local held = {}
local function press(list) for _, b in ipairs(list) do pad.force(b); held[#held + 1] = b end end
local function release_all() for _, b in ipairs(held) do pad.release(b) end; held = {} end

local loaded, last_mode, last_scene, since, done = false, -1, "", nil, false
local function on_vsync()
    if done then return end
    v = v + 1
    if not COLD and not loaded and v >= 2 then
        loaded = true
        log((sstate.load(START) and "resumed " or "FAILED to load ") .. START)
        return
    end
    local m = mem.read_u8(GM) or 255
    local sc = scene()
    if m ~= last_mode or sc ~= last_scene then
        log(string.format("vsync %d: mode 0x%02X scene=%s", v, m, sc))
        last_mode, last_scene = m, sc
    end
    if (v % 300) == 0 then
        local x, z = player_xz()
        log(string.format("...vsync %d mode 0x%02X scene=%s player (%s,%s)", v, m, sc, tostring(x), tostring(z)))
    end
    if HOLD ~= "" and not since then pad.force(pad.BTN[HOLD]) end
    if not NO_MASH then
        if (v % EVERY) == 0 then
            press(FIELDISH[m] and FIELD_BTNS or { pad.BTN.START, pad.BTN.CROSS })
        elseif (v % EVERY) == 5 then
            release_all()
        end
    end
    if at_target() then
        since = since or v
        if HOLD ~= "" then pad.release(pad.BTN[HOLD]) end
        if v - since >= SETTLE then
            release_all()
            log(string.format("target reached (mode 0x%02X scene=%s) at vsync %d", m, sc, v))
            checkpoint()
            done = true
            PCSX.quit(0)
        end
    else
        since = nil
    end
    if v >= MAX_V then log("max vsync; quitting"); done = true; PCSX.quit(0) end
end

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
log(string.format("armed: scene=%s mode=0x%02X label=%s cold=%s", SCENE_T, MODE_T, LABEL, tostring(COLD)))
