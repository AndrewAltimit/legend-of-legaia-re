-- autorun_translation_playcheck.lua
--
-- Play-time check of a translated disc: walk the game far enough that
-- every text carrier a language pack touches has been drawn once, and
-- screenshot each station so a human can read which language it shows.
-- Poll-only (no breakpoints) - run --fast. Two modes (LEGAIA_PLAYCHECK):
--
--   newgame  (default) COLD BOOT the disc under test: mash START through
--            the boot / title, then CROSS (with an UP ahead of each press,
--            so a Yes/No picker lands on its first row) through NEW GAME,
--            the name-select screen, the prologue narration and the first
--            town dialogue; on the first field-run frame (mode 3) open the
--            pause menu (SELECT), screenshot it, close it (CIRCLE) and
--            quit. This is the only session shape that shows the SCUS-
--            resident text (names, place-name cells, system strings) of
--            the disc under test: a save state carries the SCUS of the
--            disc that made it.
--   shot     load LEGAIA_SSTATE and screenshot it 90 vsyncs later as
--            shot_<LEGAIA_SHOT_STEM>.raw - the second half of a cold-boot
--            walk, whose own frames come back empty in this build.
--   state    load LEGAIA_SSTATE (a field state parked before a scripted
--            fight, e.g. --scenario v0_1_tetsu_dialogue_accept) on the disc
--            under test and mash CROSS through the fight: the battle and
--            tutorial overlays stream from the disc at battle start, so
--            their strings are the disc's even though the state's field
--            overlay and SCUS are not.
--
-- Screenshots (.raw + .raw.meta, scripts/pcsx-redux/raw2png.py converts)
-- land in LEGAIA_OUT_DIR at every game-mode or scene change and every
-- LEGAIA_SHOT_EVERY ticks; playcheck.log carries the tick / mode / scene
-- timeline. The disc must sit in a directory of its own: PCSX-Redux
-- auto-applies a sibling .ppf.
--
--   bash scripts/pcsx-redux/run_probe.sh --fast \
--     --lua scripts/pcsx-redux/autorun_translation_playcheck.lua \
--     --iso <dir>/<translated>.bin --out-dir <out> --frames 1
--   LEGAIA_PLAYCHECK=state bash scripts/pcsx-redux/run_probe.sh --fast \
--     --lua scripts/pcsx-redux/autorun_translation_playcheck.lua \
--     --iso <dir>/<translated>.bin --scenario v0_1_tetsu_dialogue_accept \
--     --out-dir <out> --frames 4000
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env = require("probe.env")
local mem = require("probe.mem")
local pad = require("probe.pad")
local sstate = require("probe.sstate")

local GM = 0x8007B83C
local SCENE_NAME = 0x8007050C
local MODE      = env.getenv("LEGAIA_PLAYCHECK", "newgame")
local OUT_DIR   = env.getenv("LEGAIA_OUT_DIR", "captures/playcheck")
local SHOT_EVERY = tonumber(env.getenv("LEGAIA_SHOT_EVERY", "300")) or 300
local MAX_TICKS  = tonumber(env.getenv("LEGAIA_MAX_TICKS", "30000")) or 30000
local MASH_UNTIL = tonumber(env.getenv("LEGAIA_MASH_UNTIL", "1400")) or 1400
local PRESS_EVERY = tonumber(env.getenv("LEGAIA_PRESS_EVERY", "50")) or 50
-- The prologue scenes (`opdeene` / `opstati` / `opurud`) run in field
-- mode 3 as well, so the pause-menu station keys on the scene, not the
-- mode: the first town the new game reaches.
local MENU_SCENE = env.getenv("LEGAIA_MENU_SCENE", "town01")
local STATE_PATH = env.getenv("LEGAIA_SSTATE", "")
-- The pause-menu button (retail: SELECT); LEGAIA_MENU_BTN overrides.
local MENU_BTN_NAME = env.getenv("LEGAIA_MENU_BTN", "SELECT")

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/playcheck.log", "w")
local function log(s)
    PCSX.log("[playcheck] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end
local function shot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(OUT_DIR .. "/" .. stem .. ".raw", "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(OUT_DIR .. "/" .. stem .. ".raw.meta", "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 228,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
end
-- Cold-boot frames come back empty from `takeScreenShot` in this build
-- (the boot-continue probe records the same), so the new-game walk also
-- writes a raw save state at every station; `shot` mode loads one and
-- screenshots it (host-gzip the .rawsstate first: `gzip -k`).
local function checkpoint(stem)
    local ok = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(OUT_DIR .. "/" .. stem .. ".rawsstate", "CREATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    log(string.format("checkpoint %s %s", stem, tostring(ok)))
end
local function read_scene()
    local s = {}
    for i = 0, 7 do
        local b = mem.read_u8(SCENE_NAME + i) or 0
        if b < 0x20 or b >= 0x7f then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end

local loaded = MODE == "newgame"
local loaded_at = 0
local LOAD_AT = tonumber(env.getenv("LEGAIA_LOAD_AT", "60")) or 60
local tick = 0
local last_mode, last_scene = -1, ""
local done = false
local held = {}          -- button -> release tick
local menu_phase = 0     -- newgame: 0 none, 1 opened, 2 closing
local menu_at = 0
local field_ticks = 0

local PAD_WORD = 0x8007B850
local pad_words = {}
local function press(btn, name, hold)
    pad.force(btn)
    held[btn] = tick + (hold or 8)
    log(string.format("tick %d: press %s (pad word 0x%04X)", tick, name,
        mem.read_u16(PAD_WORD) or 0))
end

local function on_tick()
    if done then return end
    tick = tick + 1
    for btn, until_t in pairs(held) do
        if tick >= until_t then pad.release(btn); held[btn] = nil end
    end
    local m = mem.read_u8(GM) or 255
    local sc = read_scene()
    -- Pad-word census: more than one distinct value proves the presses
    -- reach the game (a walk that "did nothing" vs buttons that never arrived).
    local pw = mem.read_u16(PAD_WORD) or 0
    if not pad_words[pw] then
        pad_words[pw] = true
        log(string.format("tick %d: pad word 0x%04X first seen", tick, pw))
    end
    if m ~= last_mode or sc ~= last_scene then
        log(string.format("tick %d: mode 0x%02X scene=%s", tick, m, sc))
        shot(string.format("t%06d_mode%02X_%s", tick, m, sc ~= "" and sc or "none"))
        if MODE == "newgame" and tick > 1 then
            checkpoint(string.format("t%06d_mode%02X_%s", tick, m, sc ~= "" and sc or "none"))
        end
        last_mode, last_scene = m, sc
    elseif tick % SHOT_EVERY == 0 then
        shot(string.format("t%06d_mode%02X_%s", tick, m, sc ~= "" and sc or "none"))
        if MODE == "newgame" then
            checkpoint(string.format("t%06d_mode%02X_%s", tick, m, sc ~= "" and sc or "none"))
        end
    end

    if MODE == "newgame" then
        if tick < MASH_UNTIL then
            -- boot + title: START every 30 ticks (never CROSS: the title
            -- menu's first row is NEW GAME and START confirms it too)
            if tick % 30 == 0 then press(pad.BTN.START, "START") end
        elseif menu_phase == 0 then
            if m == 3 and sc == MENU_SCENE then
                -- Let the town's opening dialogue play out (CROSS still
                -- advances it), then open the menu once the lines stop.
                field_ticks = field_ticks + 1
                if field_ticks < 900 then
                    local ph = tick % PRESS_EVERY
                    if ph == 20 then press(pad.BTN.CROSS, "CROSS") end
                elseif field_ticks == 990 then
                    shot(string.format("t%06d_field_%s", tick, sc))
                    checkpoint(string.format("t%06d_field_%s", tick, sc))
                    press(pad.BTN[MENU_BTN_NAME] or pad.BTN.SELECT, MENU_BTN_NAME .. " (pause menu)")
                    menu_phase = 1
                    menu_at = tick
                end
            else
                field_ticks = 0
                local ph = tick % PRESS_EVERY
                if ph == 0 then press(pad.BTN.UP, "UP")
                elseif ph == 20 then press(pad.BTN.CROSS, "CROSS") end
            end
        elseif menu_phase == 1 then
            if tick == menu_at + 120 then
                shot(string.format("t%06d_pausemenu_%s", tick, sc))
                checkpoint(string.format("t%06d_pausemenu_%s", tick, sc))
                press(pad.BTN.DOWN, "DOWN")
            elseif tick == menu_at + 160 then
                press(pad.BTN.CROSS, "CROSS (menu row 2)")
            elseif tick == menu_at + 280 then
                shot(string.format("t%06d_pausemenu_sub_%s", tick, sc))
                checkpoint(string.format("t%06d_pausemenu_sub_%s", tick, sc))
                press(pad.BTN.CIRCLE, "CIRCLE")
            elseif tick == menu_at + 320 then
                press(pad.BTN.CIRCLE, "CIRCLE")
            elseif tick == menu_at + 400 then
                shot(string.format("t%06d_field_after_menu_%s", tick, sc))
                done = true
                log("done: field reached, menu shown")
                PCSX.quit(0)
            end
        end
    elseif not loaded then
        -- A state loaded before the machine has booted leaves it frozen
        -- (the probe SM waits the same 60 vsyncs): load on the tick loop.
        if tick == LOAD_AT then
            if STATE_PATH == "" or not sstate.load(STATE_PATH) then
                log("state load failed: " .. STATE_PATH)
                PCSX.quit(2)
            end
            loaded = true
            loaded_at = tick
            log("tick " .. tick .. ": state loaded " .. STATE_PATH)
        end
    elseif MODE == "shot" then
        local el = tick - loaded_at
        if el == 90 then
            shot(string.format("shot_%s", env.getenv("LEGAIA_SHOT_STEM", "state")))
            done = true
            log("done: shot taken")
            PCSX.quit(0)
        end
    elseif MODE == "menu" then
        -- state mode, field: open the pause menu (its overlay streams from
        -- the disc under test when it opens), step into the second row,
        -- screenshot both, close.
        local el = tick - loaded_at
        if el == 60 then
            shot(string.format("t%06d_field_%s", tick, sc))
            press(pad.BTN[MENU_BTN_NAME] or pad.BTN.SELECT, MENU_BTN_NAME .. " (pause menu)")
        elseif el == 180 then
            shot(string.format("t%06d_pausemenu_%s", tick, sc))
            press(pad.BTN.DOWN, "DOWN")
        elseif el == 220 then
            press(pad.BTN.CROSS, "CROSS (menu row 2)")
        elseif el == 340 then
            shot(string.format("t%06d_pausemenu_sub_%s", tick, sc))
            press(pad.BTN.CIRCLE, "CIRCLE")
        elseif el == 380 then
            press(pad.BTN.CIRCLE, "CIRCLE")
        elseif el == 460 then
            done = true
            log("done: menu shown")
            PCSX.quit(0)
        end
    else
        -- state mode: CROSS through dialogue / battle prompts
        local ph = (tick - loaded_at) % PRESS_EVERY
        if ph == 0 then press(pad.BTN.CROSS, "CROSS") end
    end
    if tick >= MAX_TICKS then
        done = true
        log("max ticks; quitting")
        shot(string.format("t%06d_timeout", tick))
        PCSX.quit(0)
    end
end

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_tick)
log("vsync driver armed; mode=" .. MODE)
