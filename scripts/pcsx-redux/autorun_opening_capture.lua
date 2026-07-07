-- autorun_opening_capture.lua
--
-- RETAIL ground-truth frame capture of the entire New-Game opening sequence:
-- cold boot -> title -> NEW GAME -> opening prologue narration ("opdeene",
-- creation-myth captions) -> prologue chain (opstati/opurud/map01) -> Rim Elm
-- ("town01" establishing sweep + Vahn walk-out + name entry).
--
-- Modeled on autorun_play_from_boot.lua (bespoke tick driver, NOT probe.run):
-- the title's XA-BGM streaming blinds the GPU::Vsync listener, so everything
-- is driven from two per-frame EXEC breakpoints (needs -interpreter -debugger,
-- and cold boot needs -fastboot; see docs/tooling/playthrough-coverage.md):
--   * FUN_801DD35C (title tick)  - cycles a START / UP / CROSS pulse pattern
--     through the logos + "PRESS START" gate + title menu. UP biases the menu
--     cursor to row 0 = NEW GAME (the pcsx-redux memcard HAS Legaia saves, so
--     the cursor may default to CONTINUE - a bare CROSS mash would load one).
--   * FUN_8001698C (field tick)  - once game_mode goes 3 (field-RUN) while
--     the scene is still "opdeene" (NB the scene-name buffer statically reads
--     "opdeene" from exe load, so the name alone is NOT a trigger), ALL input
--     stops (the narration presentation must be captured with natural,
--     unmashed timing).
--
-- Screenshots are PACED by the field tick but EXECUTED via PCSX.nextTick
-- (main polling loop), never in the exec-BP callback itself:
-- PCSX.GPU.takeScreenShot() called repeatedly from the CPU/BP context races
-- the renderer and segfaults the emulator after a few dozen shots (observed
-- on this exact flow). nextTick also sidesteps the title-XA vsync-blind
-- problem - GPU::Vsync delivery is NOT required for the shots to fire.
--
-- Passive-with-assist policy: each scene gets LEGAIA_STALL_TICKS of untouched
-- observation; only if the scene has not changed by then does a gentle CROSS
-- pulse start (logged, so any frame after "assist" is known to be driven).
-- town01 is terminal: never mashed (the name-entry screen would eat inputs);
-- capture runs LEGAIA_TOWN_TICKS there, then the probe quits.
--
-- Wrong-path guard: if a scene other than "opdeene" loads while still in the
-- TITLE phase, the mash confirmed CONTINUE and loaded a memory-card save; the
-- probe logs OPENING_CAPTURE_WRONG_PATH and quits non-zero so the caller can
-- adjust the pulse pattern instead of analyzing a bogus capture.
--
-- Env vars:
--   LEGAIA_OUT_DIR     output dir (shots/ + shots.csv + opening.log land here)
--   LEGAIA_CAP_EVERY   field ticks between screenshots (default 12)
--   LEGAIA_STALL_TICKS passive window per scene before assist-mash (default 4200)
--   LEGAIA_TOWN_TICKS  capture length once town01 is active (default 5400)
--   LEGAIA_MAX_TICKS   global field-tick safety cap (default 45000)
--   LEGAIA_TITLE_MAX   title-tick give-up cap (default 40000)
--   LEGAIA_MASH_EVERY  ticks between title pulses (default 20)
--
-- Output:
--   <OUT_DIR>/opening.log    timeline (scene changes, assist onset, errors)
--   <OUT_DIR>/shots.csv      vsync,tick,scene,mode,file manifest
--   <OUT_DIR>/shots/shot_<tick>.screen(+.meta)  raw framebuffer dumps;
--       decode with scripts/pcsx-redux/decode_pcsx_screen.py
--
-- Run (cold boot; NO save state):
--   timeout --kill-after=15s 2400 ~/Tools/pcsx-redux/pcsx-redux \
--     -interpreter -debugger -fastboot -bios ~/.mednafen/firmware/SCPH1001.BIN \
--     -iso "$LEGAIA_DISC_BIN" -run -stdout \
--     -dofile scripts/pcsx-redux/autorun_opening_capture.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env = require("probe.env")
local mem = require("probe.mem")
local pad = require("probe.pad")
local bp  = require("probe.bp")

local GM         = 0x8007B83C -- game_mode (low byte)
local SCENE_NAME = 0x8007050C -- active scene-name buffer ("opdeene", "town01", ...)
local TITLE_BP   = 0x801DD35C -- title overlay per-frame tick
local FIELD_BP   = 0x8001698C -- default mode handler per-frame vsync-sync

local OUT_DIR     = env.getenv("LEGAIA_OUT_DIR", "captures/opening_capture")
local CAP_EVERY   = tonumber(env.getenv("LEGAIA_CAP_EVERY", "12")) or 12
local STALL_TICKS = tonumber(env.getenv("LEGAIA_STALL_TICKS", "4200")) or 4200
local TOWN_TICKS  = tonumber(env.getenv("LEGAIA_TOWN_TICKS", "5400")) or 5400
local MAX_TICKS   = tonumber(env.getenv("LEGAIA_MAX_TICKS", "45000")) or 45000
local TITLE_MAX   = tonumber(env.getenv("LEGAIA_TITLE_MAX", "40000")) or 40000
local MASH_EVERY  = tonumber(env.getenv("LEGAIA_MASH_EVERY", "20")) or 20

os.execute(string.format("mkdir -p %q", OUT_DIR .. "/shots"))
local LOG = io.open(OUT_DIR .. "/opening.log", "w")
local CSV = io.open(OUT_DIR .. "/shots.csv", "w")
if CSV then CSV:write("vsync,tick,scene,mode,file\n") end

local function log(s)
    PCSX.log("[opening] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end

local function read_mode()
    if not mem.in_ram(GM) then return nil end
    return mem.read_u8(GM)
end

local function read_scene()
    if not mem.in_ram(SCENE_NAME) then return "" end
    local s = {}
    for i = 0, 7 do
        local b = mem.read_u8(SCENE_NAME + i) or 0
        if b < 0x20 or b >= 0x7f then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end

-- ---------------------------------------------------------------- screenshots
-- Executed ONLY via PCSX.nextTick (main-loop context; BP-context screenshots
-- segfault - see header). Named by the field tick at schedule time.
local g_vsync = 0
local g_shot_pending = false
local g_last_shot_tick = 0
local function take_shot(tick)
    g_shot_pending = false
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil then
        log(string.format("tick %d: takeScreenShot unavailable", tick))
        return
    end
    local w, h = tonumber(ss.width), tonumber(ss.height)
    -- ss.bpp is an enum: 0 = 16 bpp BGR555, nonzero = 24 bpp BGR888
    local bpp = (tonumber(ss.bpp) or 0) ~= 0 and 24 or 16
    local rel = string.format("shots/shot_%07d.screen", tick)
    local fh = io.open(OUT_DIR .. "/" .. rel, "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data))
    fh:close()
    local mh = io.open(OUT_DIR .. "/" .. rel .. ".meta", "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n", w, h, bpp))
        mh:close()
    end
    g_last_shot_tick = tick
    if CSV then
        CSV:write(string.format("%d,%d,%s,0x%02X,%s\n",
            g_vsync, tick, read_scene(), read_mode() or 0xFF, rel))
        CSV:flush()
    end
end

local function schedule_shot(tick)
    if g_shot_pending then return end
    g_shot_pending = true
    PCSX.nextTick(function() take_shot(tick) end)
end

-- ---------------------------------------------------------------- state
local PHASE = "TITLE" -- TITLE -> CAPTURE -> DONE
local g_title_tick = 0
local g_tick = 0            -- field ticks
local g_pulse = 0           -- title pulse counter
local g_release_at = 0      -- title: tick to release held buttons
local g_held = {}           -- currently forced buttons
-- capture-phase state
local cur_scene = ""
local scene_enter_tick = 0
local assist = false
local g_assist_hold = 0
local g_quit_at = nil

local function hold(btn)
    pad.force(btn)
    g_held[#g_held + 1] = btn
end
local function release_all()
    for _, b in ipairs(g_held) do pad.release(b) end
    g_held = {}
end

local function finish(code, why)
    if PHASE == "DONE" then return end
    PHASE = "DONE"
    release_all()
    log("done: " .. why)
    log(string.format("=== summary === shots in %s/shots, manifest shots.csv", OUT_DIR))
    if LOG then LOG:close() end
    if CSV then CSV:close() end
    g_quit_at = { code = code, at = g_tick + 2, title_at = g_title_tick + 2 }
end

local function enter_capture(from)
    release_all()
    PHASE = "CAPTURE"
    cur_scene = read_scene()
    scene_enter_tick = g_tick
    log(string.format("CAPTURE start (via %s tick): scene=%q mode=0x%02X title_tick=%d field_tick=%d",
        from, cur_scene, read_mode() or 0xFF, g_title_tick, g_tick))
end

-- Title pulse pattern: START (press-start gate / FMV skip), UP (bias the menu
-- cursor to row 0 = NEW GAME), CROSS (confirm). Cycled one per MASH_EVERY.
local PATTERN = { { pad.BTN.START }, { pad.BTN.UP }, { pad.BTN.CROSS } }

-- NB: the scene-name buffer statically contains "opdeene" from exe load (it is
-- the SCUS default), so scene name alone is NOT an opening trigger. The real
-- trigger is game_mode == 3 (field-RUN) while the scene is still "opdeene".
local function opening_reached()
    return read_mode() == 3 and read_scene() == "opdeene"
end

local function title_tick()
    g_title_tick = g_title_tick + 1
    if PHASE == "DONE" then
        if g_quit_at and g_title_tick >= g_quit_at.title_at then PCSX.quit(g_quit_at.code) end
        return
    end
    if PHASE ~= "TITLE" then return end
    local scene = read_scene()
    if opening_reached() then enter_capture("title"); return end
    if scene ~= "opdeene" and scene ~= "" and read_mode() == 3 then
        log(string.format("OPENING_CAPTURE_WRONG_PATH: scene %q loaded from title (CONTINUE?)", scene))
        finish(1, "wrong path")
        return
    end
    if g_title_tick >= TITLE_MAX then
        log("OPENING_CAPTURE_TITLE_TIMEOUT")
        finish(1, "title timeout")
        return
    end
    if (g_title_tick % 300) == 0 then
        log(string.format("...title tick %d mode=0x%02X scene=%q pulse=%d",
            g_title_tick, read_mode() or 0xFF, scene, g_pulse))
    end
    if g_release_at > 0 and g_title_tick >= g_release_at then
        release_all()
        g_release_at = 0
    elseif g_release_at == 0 and (g_title_tick % MASH_EVERY) == 0 then
        g_pulse = g_pulse + 1
        for _, b in ipairs(PATTERN[(g_pulse % #PATTERN) + 1]) do hold(b) end
        g_release_at = g_title_tick + 5
    end
end

local function field_tick()
    g_tick = g_tick + 1
    if PHASE == "DONE" then
        if g_quit_at and g_tick >= g_quit_at.at then PCSX.quit(g_quit_at.code) end
        return
    end
    if PHASE == "TITLE" then
        -- transition detection only; the title tick owns all input here
        if opening_reached() then enter_capture("field") end
        return
    end
    -- PHASE == "CAPTURE"
    local scene = read_scene()
    if scene ~= cur_scene and scene ~= "" then
        log(string.format("tick %d: scene %q -> %q (mode 0x%02X)",
            g_tick, cur_scene, scene, read_mode() or 0xFF))
        cur_scene = scene
        scene_enter_tick = g_tick
        assist = false
        release_all()
    end
    if (g_tick % CAP_EVERY) == 0 then schedule_shot(g_tick) end
    -- stalled-shot detection: nextTick shots should land within a few ticks
    if g_last_shot_tick > 0 and g_tick - g_last_shot_tick > 300 then
        log(string.format("WARN tick %d: shots stalled (last landed at tick %d)",
            g_tick, g_last_shot_tick))
        g_last_shot_tick = g_tick -- rate-limit the warning
    end
    if (g_tick % 600) == 0 then
        log(string.format("...field tick %d scene=%q mode=0x%02X assist=%s",
            g_tick, scene, read_mode() or 0xFF, tostring(assist)))
    end
    if g_tick >= MAX_TICKS then finish(0, "global tick cap"); return end
    if scene == "town01" then
        if g_tick - scene_enter_tick >= TOWN_TICKS then
            finish(0, "town01 capture window complete")
        end
        return -- terminal scene: never assist-mash (name entry eats inputs)
    end
    -- assist-mash: only after a full natural observation window in this scene
    if not assist and g_tick - scene_enter_tick >= STALL_TICKS then
        assist = true
        log(string.format("tick %d: scene %q stalled %d ticks; starting CROSS assist",
            g_tick, scene, STALL_TICKS))
    end
    if assist then
        if g_assist_hold > 0 and g_tick >= g_assist_hold then
            release_all(); g_assist_hold = 0
        elseif g_assist_hold == 0 and (g_tick % 40) == 0 then
            hold(pad.BTN.CROSS)
            g_assist_hold = g_tick + 5
        end
    end
end

pcall(function() bp.arm(TITLE_BP, "Exec", 4, "title_tick", title_tick) end)
pcall(function() bp.arm(FIELD_BP, "Exec", 4, "field_tick", field_tick) end)
log(string.format("opening capture armed: out=%s cap_every=%d stall=%d town=%d",
    OUT_DIR, CAP_EVERY, STALL_TICKS, TOWN_TICKS))

-- The vsync listener is a heartbeat + backup quit path only (it goes blind
-- during XA streaming; the exec-bp ticks + nextTick shots are the drivers).
-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", function()
    g_vsync = g_vsync + 1
    if PHASE == "DONE" and g_quit_at then
        -- backup quit path in case both tick BPs have stopped firing
        g_quit_at.vs_seen = (g_quit_at.vs_seen or 0) + 1
        if g_quit_at.vs_seen > 10 then PCSX.quit(g_quit_at.code) end
    end
    if (g_vsync % 1200) == 0 then
        PCSX.log(string.format("[opening] vsync heartbeat %d phase=%s", g_vsync, PHASE))
    end
end)
