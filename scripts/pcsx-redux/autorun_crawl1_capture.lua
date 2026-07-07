-- autorun_crawl1_capture.lua
--
-- FOCUSED retail ground-truth capture of opdeene *crawl-1* (the first
-- creation-myth narration crawl, "God created the heavens, the earth, and the
-- seas...") - the beat the engine port frames as a flat gold "creation void"
-- and which lacks a pinned retail framebuffer. Complements the whole-opening
-- autorun_opening_capture.lua by stopping at the end of opdeene instead of
-- driving all the way to town01, so the run is short and the opdeene frames are
-- densely sampled.
--
-- Same two-EXEC-BP driver as autorun_opening_capture.lua (the title XA-BGM
-- blinds GPU::Vsync, so nothing can be paced off vsync; needs -interpreter
-- -debugger -fastboot):
--   * FUN_801DD35C (title tick) - START / UP / CROSS pulse pattern through the
--     logos + PRESS START gate + title menu (UP biases the cursor to row 0 =
--     NEW GAME so a stray CROSS doesn't load a memory-card save).
--   * FUN_8001698C (field tick) - once game_mode == 3 (field-RUN) while the
--     scene is "opdeene", ALL input stops and dense screenshots begin. The
--     crawl must be captured with natural, unmashed timing.
--
-- Screenshots are scheduled from the field tick but EXECUTED via PCSX.nextTick
-- (main-loop context): PCSX.GPU.takeScreenShot() from a BP callback races the
-- renderer and segfaults after a few dozen shots.
--
-- Terminates when opdeene has been active for LEGAIA_OPDEENE_TICKS field ticks
-- (covers crawl-1 through the pan toward the grove reveal), OR when the scene
-- advances past opdeene (opstati), whichever comes first. No assist-mash: the
-- whole opdeene leg is input-free in retail.
--
-- Wrong-path guard: a non-opdeene scene reaching mode 3 straight from the title
-- means the mash confirmed CONTINUE and loaded a save; logged + non-zero quit.
--
-- Env vars:
--   LEGAIA_OUT_DIR       output dir (shots/ + shots.csv + crawl1.log land here)
--   LEGAIA_CAP_EVERY     field ticks between screenshots (default 8; dense)
--   LEGAIA_OPDEENE_TICKS opdeene field-tick budget before quit (default 2600)
--   LEGAIA_TITLE_MAX     title-tick give-up cap (default 40000)
--   LEGAIA_MASH_EVERY    ticks between title pulses (default 20)
--
-- Output:
--   <OUT_DIR>/crawl1.log   timeline (scene changes, quit reason, errors)
--   <OUT_DIR>/shots.csv    vsync,tick,scene,mode,file manifest
--   <OUT_DIR>/shots/shot_<tick>.screen(+.meta)  raw framebuffer dumps;
--       decode with scripts/pcsx-redux/decode_pcsx_screen.py
--
-- Run (cold boot; NO save state):
--   timeout --kill-after=15s 900 ~/Tools/pcsx-redux/pcsx-redux \
--     -interpreter -debugger -fastboot -bios ~/.mednafen/firmware/SCPH1001.BIN \
--     -iso "$LEGAIA_DISC_BIN" -run -stdout \
--     -dofile scripts/pcsx-redux/autorun_crawl1_capture.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env = require("probe.env")
local mem = require("probe.mem")
local pad = require("probe.pad")
local bp  = require("probe.bp")

local GM         = 0x8007B83C -- game_mode (low byte)
local SCENE_NAME = 0x8007050C -- active scene-name buffer ("opdeene", "town01", ...)
local TITLE_BP   = 0x801DD35C -- title overlay per-frame tick
local FIELD_BP   = 0x8001698C -- default mode handler per-frame vsync-sync

local OUT_DIR       = env.getenv("LEGAIA_OUT_DIR", "captures/crawl1_capture")
local CAP_EVERY     = tonumber(env.getenv("LEGAIA_CAP_EVERY", "8")) or 8
local OPDEENE_TICKS = tonumber(env.getenv("LEGAIA_OPDEENE_TICKS", "2600")) or 2600
local TITLE_MAX     = tonumber(env.getenv("LEGAIA_TITLE_MAX", "40000")) or 40000
local MASH_EVERY    = tonumber(env.getenv("LEGAIA_MASH_EVERY", "20")) or 20

os.execute(string.format("mkdir -p %q", OUT_DIR .. "/shots"))
local LOG = io.open(OUT_DIR .. "/crawl1.log", "w")
local CSV = io.open(OUT_DIR .. "/shots.csv", "w")
if CSV then CSV:write("vsync,tick,scene,mode,file\n") end

local function log(s)
    PCSX.log("[crawl1] " .. s)
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
local cur_scene = ""
local opdeene_enter_tick = 0
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
    opdeene_enter_tick = g_tick
    log(string.format("CAPTURE start (via %s tick): scene=%q mode=0x%02X title_tick=%d field_tick=%d",
        from, cur_scene, read_mode() or 0xFF, g_title_tick, g_tick))
end

-- Title pulse pattern: START (press-start gate / FMV skip), UP (bias the menu
-- cursor to row 0 = NEW GAME), CROSS (confirm). Cycled one per MASH_EVERY.
local PATTERN = { { pad.BTN.START }, { pad.BTN.UP }, { pad.BTN.CROSS } }

-- The scene-name buffer statically contains "opdeene" from exe load, so scene
-- name alone is NOT a trigger; the real trigger is game_mode == 3 (field-RUN).
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
        log(string.format("CRAWL1_CAPTURE_WRONG_PATH: scene %q loaded from title (CONTINUE?)", scene))
        finish(1, "wrong path")
        return
    end
    if g_title_tick >= TITLE_MAX then
        log("CRAWL1_CAPTURE_TITLE_TIMEOUT")
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
        if opening_reached() then enter_capture("field") end
        return
    end
    -- PHASE == "CAPTURE" (opdeene)
    local scene = read_scene()
    if scene ~= cur_scene and scene ~= "" then
        log(string.format("tick %d: scene %q -> %q (mode 0x%02X)",
            g_tick, cur_scene, scene, read_mode() or 0xFF))
        -- opdeene has advanced (opstati/...) - crawl-1 leg is complete.
        finish(0, string.format("opdeene ended -> %q", scene))
        return
    end
    if (g_tick % CAP_EVERY) == 0 then schedule_shot(g_tick) end
    if g_last_shot_tick > 0 and g_tick - g_last_shot_tick > 300 then
        log(string.format("WARN tick %d: shots stalled (last landed at tick %d)",
            g_tick, g_last_shot_tick))
        g_last_shot_tick = g_tick
    end
    if (g_tick % 300) == 0 then
        log(string.format("...opdeene field tick %d (%d since enter) mode=0x%02X",
            g_tick, g_tick - opdeene_enter_tick, read_mode() or 0xFF))
    end
    if g_tick - opdeene_enter_tick >= OPDEENE_TICKS then
        finish(0, string.format("opdeene tick budget %d reached", OPDEENE_TICKS))
    end
end

pcall(function() bp.arm(TITLE_BP, "Exec", 4, "title_tick", title_tick) end)
pcall(function() bp.arm(FIELD_BP, "Exec", 4, "field_tick", field_tick) end)
log(string.format("crawl1 capture armed: out=%s cap_every=%d opdeene_ticks=%d",
    OUT_DIR, CAP_EVERY, OPDEENE_TICKS))

-- Vsync listener = heartbeat + backup quit path only (goes blind during XA).
-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", function()
    g_vsync = g_vsync + 1
    if PHASE == "DONE" and g_quit_at then
        g_quit_at.vs_seen = (g_quit_at.vs_seen or 0) + 1
        if g_quit_at.vs_seen > 10 then PCSX.quit(g_quit_at.code) end
    end
    if (g_vsync % 1200) == 0 then
        PCSX.log(string.format("[crawl1] vsync heartbeat %d phase=%s", g_vsync, PHASE))
    end
end)
