-- autorun_record_inputs.lua
--
-- INTERACTIVE input recorder. Resumes a save state and records the player's live
-- button input as a timeline, so a manually-played sequence (e.g. walk to Tetsu,
-- pick the 3rd training-fight option, start the spar) can be replayed
-- deterministically by autorun_replay_inputs.lua to capture the result.
--
-- It records the per-frame button mask 0x8007B850 (built by FUN_8001822C before
-- the field tick; pre-remap at the field-tick entry, and un-remapped while a
-- dialogue is up) once per field tick, writing a CSV row each time the mask
-- changes: `frame,held_hex`. Frame 0 is the first field tick after the save
-- loads, so record + replay share one clock. It auto-quits a few seconds after a
-- battle starts (game_mode 0x8007B83C == 0x15) so the recording cleanly spans the
-- spar start; you can also just close the window (every row is flushed).
--
-- RUN IT INTERACTIVELY (real window + keyboard - NOT under xvfb), e.g.:
--   bash scripts/pcsx-redux/run_probe.sh --scenario s4_rimelm_door_transition \
--        --lua scripts/pcsx-redux/autorun_record_inputs.lua
-- then play. The CSV path is printed as "[rec] inputs -> <path>".
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_BOOT_DELAY.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP=0x8001698C; local GM=0x8007B83C; local SCENE_NAME=0x8007050C
local HELD=0x8007B850; local EDGE=0x8007B874
local BATTLE_MODE=0x15

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/input_record")
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2
local QUIT_AFTER = tonumber(env.getenv("LEGAIA_QUIT_AFTER", "180")) or 180
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "120000")) or 120000

os.execute(string.format("mkdir -p %q", OUT_DIR))
local CSV = OUT_DIR .. "/inputs.csv"
local F = io.open(CSV, "w")
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or 0 end
local function ru16(a) return mem.in_ram(a) and mem.read_u16(a) or 0 end
local function read_scene()
    local s={}; for i=0,7 do local b=ru8(SCENE_NAME+i); if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end
local function emit(s) if F then F:write(s.."\n"); F:flush() end end

local vsync, loaded = 0, false
local frame, last_held = -1, nil
local battle_at = nil

-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync=vsync+1
    if not loaded and START_SAVE~="" and vsync>=START_DELAY then
        loaded=true
        local ok=sstate.load(START_SAVE)
        PCSX.log("[rec] "..(ok and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE)))
        PCSX.log("[rec] inputs -> "..CSV)
        emit("# record_inputs save="..START_SAVE)
        emit("# columns: frame,held_hex (held = 0x8007B850 per-frame button mask)")
        return
    end
    -- once a battle started, the field tick stops; finish the quit countdown here
    if battle_at~=nil then
        QUIT_AFTER = QUIT_AFTER - 1
        if QUIT_AFTER<=0 then
            emit("# end")
            if F then F:close(); F=nil end
            PCSX.log("[rec] done; CSV at "..CSV)
            PCSX.quit(0)
        end
    end
end)

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    if not loaded then return end
    frame=frame+1
    if frame==0 then
        emit(string.format("# frame0 scene=%q mode=0x%02X", read_scene(), ru8(GM)))
    end
    if frame>=MAX_FRAMES then emit("# end (max_frames)"); if F then F:close(); F=nil end; PCSX.log("[rec] max_frames; CSV at "..CSV); PCSX.quit(0); return end
    local held=ru16(HELD)
    if held~=last_held then
        emit(string.format("%d,0x%04X", frame, held))
        last_held=held
        PCSX.log(string.format("[rec] f%d held=0x%04X edge=0x%04X", frame, held, ru16(EDGE)))
    end
    if ru8(GM)==BATTLE_MODE and battle_at==nil then
        battle_at=frame
        emit(string.format("# BATTLE at frame %d (mode 0x15)", frame))
        PCSX.log(string.format("[rec] *** BATTLE at f%d - recording %d more frames then quitting ***", frame, QUIT_AFTER))
    end
end)

PCSX.log("[rec] record_inputs armed - resume, then PLAY (walk to Tetsu, do the spar). CSV: "..CSV)
