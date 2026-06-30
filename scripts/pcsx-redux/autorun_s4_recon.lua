-- autorun_s4_recon.lua
--
-- Recon free-roam navigation from the s3_rimelm_freeroam anchor toward the first
-- scene transition (S4). Resumes S3 and sweeps the d-pad (each direction held
-- for a window), logging the active scene name (0x8007050C), game_mode, player
-- position (player+0x14/+0x18) and the engaged flag, and flags any scene-name
-- change (a transition / door). Tells us which direction reaches an exit/door.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_PHASE_LEN, LEGAIA_MAX_FRAMES.

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
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s4_recon")
local PHASE_LEN  = tonumber(env.getenv("LEGAIA_PHASE_LEN", "240")) or 240
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "2600")) or 2600
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/recon.log", "w")
local function log(s) PCSX.log("[s4] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end
local function s32(v)  if v == nil then return nil end; if v >= 0x80000000 then return v - 0x100000000 end; return v end
local function read_scene()
    local s = {}
    for i = 0, 7 do local b = ru8(SCENE_NAME+i) or 0; if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end

local vsync, loaded = 0, false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync = vsync + 1
    if not loaded and START_SAVE ~= "" and vsync >= START_DELAY then
        loaded = true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
end)

local DIRS = { "UP", "RIGHT", "DOWN", "LEFT" }
local frame, held_dir, last_scene = 0, nil, nil
bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame = frame + 1
    -- which sweep phase (after a short settle)
    local settle = 30
    local phase = math.floor((frame - settle) / PHASE_LEN)
    local dir = (frame >= settle) and DIRS[(phase % #DIRS) + 1] or nil

    -- (re)apply held direction
    if dir ~= held_dir then
        if held_dir then pad.release(pad.BTN[held_dir]) end
        held_dir = dir
        if dir then pad.force(pad.BTN[dir]); log(string.format("[f%d] hold %s", frame, dir)) end
    end

    local sc = read_scene()
    if last_scene ~= nil and sc ~= last_scene then
        log(string.format("*** [f%d] SCENE CHANGE %q -> %q (mode=0x%02X) ***", frame, last_scene, sc, ru8(GM) or 0xFF))
    end
    last_scene = sc

    if (frame % 40) == 0 then
        local pp = ru32(PLAYER)
        log(string.format("[f%d] dir=%s scene=%q mode=0x%02X pos=(%s,%s)",
            frame, tostring(dir), sc, ru8(GM) or 0xFF,
            pp and tostring(s32(ru32(pp+0x14))) or "nil", pp and tostring(s32(ru32(pp+0x18))) or "nil"))
    end

    if frame >= MAX_FRAMES then
        if held_dir then pad.release(pad.BTN[held_dir]) end
        if LOG then LOG:close() end; PCSX.quit(0)
    end
end)

log("s4 recon armed (direction sweep)")
