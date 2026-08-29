-- autorun_battle_mesh_dump.lua
--
-- Cold-boot -> MEMORY CARD load -> forced battle -> full RAM dump.
--
-- The point of the memory card (rather than a save state) is that a save
-- state carries a stale RAM image: it would replay the meshes that were
-- resident when the state was taken, which is exactly the data a disc
-- patch changes. Booting the patched disc and loading a card forces the
-- game to read every asset off the disc under test, so what lands in RAM
-- is what the patch actually produced.
--
-- The Lua side stays deliberately thin - navigate, trigger, dump, quit -
-- because everything here runs inside a fragile breakpoint context. All
-- parsing and comparison happens offline against the 2 MiB image (see
-- scripts/pcsx-redux/decode_battle_mesh.py).
--
-- Navigation notes, both hard-won and both load-bearing:
--   * The title's XA-BGM streaming stops GPU::Vsync delivery to an
--     autorun and does NOT resume through the field load, so EXEC
--     BREAKPOINTS are the frame clock, not a vsync listener.
--   * The title menu is driven by SCRIPTED taps, not by steering on a
--     cursor word. The documented cursor (title_state[+0x1FC], base
--     0x801F0000) reads 0 throughout, and the one nearby word that did
--     move turned out to be a free-running mod-3 counter. The menu
--     opens on NEW GAME (row 0), so N spaced DOWN taps select row N.
--
-- Env vars:
--   LEGAIA_OUT_DIR    output directory (run_probe.sh sets this)
--   LEGAIA_FORMATION  comma-separated monster ids to force (default
--                     "162" = Gi Delilas). Empty = wait for a random
--                     encounter instead of forcing one.
--   LEGAIA_BATTLE_SETTLE  battle frames to let the load finish before
--                     dumping (default 240).
--   LEGAIA_MAX_FRAMES safety cap on tick count (default 60000).
--
-- Output (under LEGAIA_OUT_DIR):
--   ram.bin           2 MiB main RAM at the dump point
--   dump.log          phase timeline + the pointers the decoder needs
--   shot.screen(.meta) framebuffer grab (decode_pcsx_screen.py)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env = require("probe.env")
local mem = require("probe.mem")
local pad = require("probe.pad")
local bp  = require("probe.bp")

local GM          = 0x8007B83C -- game_mode (low byte)
local SCENE_NAME  = 0x8007050C -- active scene-name buffer
local TITLE_STATE = 0x801F0000 -- title-overlay state struct
local TITLE_ROW   = TITLE_STATE + 0x1FC -- live menu cursor (0 = NEW GAME)
local TITLE_CURY  = TITLE_STATE + 0x1F8 -- cursor Y the menu state quantises
local TITLE_SUB   = TITLE_STATE + 0x204 -- sub-mode selector (25-slot JT)
local TITLE_MENU_MODE = 0x17 -- game_mode while the title menu is up
local MENU_TAP        = 24   -- ticks between menu taps
local MENU_SETTLE     = 120  -- ticks to let the menu animate in first
local CURSOR_CANDIDATES = {
    0x801F01F4, 0x801F01F8, 0x801F01FC, 0x801F0200, 0x801F0204,
}
local FORMATION   = 0x8007BD0C -- 4 monster id bytes
local PARTY_SLOTS = 0x8007BD10 -- per-slot active member id
local TITLE_BP    = 0x801DD35C -- title tick
local FIELD_BP    = 0x8001698C -- default mode handler per-frame
local BATTLE_BP   = 0x80048A08 -- battle per-actor draw

local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/battle_mesh")
local SETTLE     = tonumber(env.getenv("LEGAIA_BATTLE_SETTLE", "240")) or 240
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "60000")) or 60000
local FORMATION_S = env.getenv("LEGAIA_FORMATION", "162")
-- Title-menu row to confirm. Row 0 is NEW GAME; CONTINUE is 1.
local MENU_ROW = tonumber(env.getenv("LEGAIA_MENU_ROW", "1")) or 1

local formation = {}
for tok in FORMATION_S:gmatch("[^,]+") do
    local v = tonumber(tok)
    if v then formation[#formation + 1] = v end
end

local log_lines = {}
local function log(fmt, ...)
    local line = string.format(fmt, ...)
    log_lines[#log_lines + 1] = line
    PCSX.log("[meshdump] " .. line)
end

local function write_file(name, data)
    local f = io.open(OUT_DIR .. "/" .. name, "wb")
    if not f then
        log("FAILED to open %s/%s for writing", OUT_DIR, name)
        return false
    end
    f:write(data)
    f:close()
    return true
end

local function scene()
    local s = mem.read_bytes(SCENE_NAME, 8)
    return tostring(s):gsub("%z.*", "")
end

local function mode()
    return mem.read_u8(GM)
end

-- Phases: TITLE -> LOAD -> FIELD -> BATTLE -> DONE.
local phase = "TITLE"
local menu_ticks = 0
local ticks = 0
local phase_ticks = 0
local battle_ticks = 0
local done = false

local function set_phase(p)
    log("phase %s -> %s (tick %d, mode 0x%02x, scene '%s')", phase, p, ticks, mode(), scene())
    phase = p
    phase_ticks = 0
end

-- One button pulse every PULSE ticks, released on the following tick, so
-- menus see clean edges rather than a held button.
local PULSE = 12
local function mash(button)
    local slot = phase_ticks % PULSE
    if slot == 0 then
        pad.force(button)
    elseif slot == 3 then
        pad.release(button)
    end
end

-- Save-browser input. UNRESOLVED, and the one step between this probe
-- and an automated card load: the run reaches the save's confirm prompt
-- ("Do you wish to load?", with the save's own summary drawn - see the
-- captured framebuffer) and then stalls there forever. CROSS alone,
-- UP+CROSS and UP+LEFT+CROSS have all been tried; none answers it, so
-- either the answer widget wants a button this cycle does not send or
-- the confirm is not reached by the pad path being driven here. Until
-- that is settled, a run still reaches a battle by NEW GAME (set
-- LEGAIA_MENU_ROW=0), which is a real cold-boot read of the disc under
-- test - just not the late-game save a card gets you.
local function browse()
    local slot = phase_ticks % (PULSE * 3)
    if slot == 0 then
        -- Both axes at once: the answer widget's "yes" is the first
        -- option whether the pair is stacked or side by side, and the
        -- staged card holds a single save so a stray grid move is
        -- recoverable.
        pad.force(pad.BTN.UP)
        pad.force(pad.BTN.LEFT)
    elseif slot == 4 then
        pad.release(pad.BTN.UP)
        pad.release(pad.BTN.LEFT)
    elseif slot == PULSE then
        pad.force(pad.BTN.CROSS)
    elseif slot == PULSE + 4 then
        pad.release(pad.BTN.CROSS)
    elseif slot == PULSE * 2 then
        pad.force(pad.BTN.CROSS)
    elseif slot == PULSE * 2 + 4 then
        pad.release(pad.BTN.CROSS)
    end
end

local function dump()
    done = true
    log("dumping at mode 0x%02x scene '%s'", mode(), scene())
    -- Pointers the offline decoder needs; recorded rather than followed
    -- here, so a layout surprise costs a re-decode and not a re-run.
    log("party_base_index=0x%08x", mem.read_u32(0x8007B824))
    for slot = 0, 2 do
        log("slot %d member=%d rec0=0x%08x", slot,
            mem.read_u8(PARTY_SLOTS + slot), mem.read_u32(0x801C9360 + slot * 4))
        log("slot %d actor=0x%08x", slot, mem.read_u32(0x801C9370 + slot * 4))
    end
    for i = 0, 7 do
        log("tmd_table[%d]=0x%08x", i, mem.read_u32(0x8007C018 + i * 4))
    end

    -- A single 2 MiB readAt permanently degrades vsync delivery, so it is
    -- the last thing this probe does before quitting.
    local ok, err = pcall(function()
        write_file("ram.bin", tostring(mem.read_bytes(0x80000000, 2 * 1024 * 1024)))
    end)
    if not ok then log("RAM dump failed: %s", tostring(err)) end

    pcall(function()
        local ss = PCSX.GPU.takeScreenShot()
        write_file("shot.screen", tostring(ss.data))
        write_file("shot.screen.meta", string.format(
            "width=%d\nheight=%d\nbpp=%d\n", ss.width, ss.height, ss.bpp))
    end)

    write_file("dump.log", table.concat(log_lines, "\n") .. "\n")
    bp.disarm()
    PCSX.quit(0)
end

local function tick()
    if done then return end
    ticks = ticks + 1
    phase_ticks = phase_ticks + 1
    if ticks > MAX_FRAMES then
        log("MAX_FRAMES reached in phase %s - dumping anyway", phase)
        dump()
        return
    end

    local m = mode()

    if phase == "TITLE" then
        -- `game_mode` is the usable title signal here: 0x10/0x11 while
        -- the attract runs, 0x17 once the menu is up. (It is NOT a
        -- "are we past the title" test - it already reads 0x10 on the
        -- first tick, which is what sent an earlier version of this
        -- probe CROSS-mashing straight into NEW GAME.) The candidate
        -- words are logged so a later run can still pin a real cursor.
        local sub = mem.read_u32(TITLE_SUB)
        if phase_ticks % 120 == 1 then
            local w = {}
            for _, va in ipairs(CURSOR_CANDIDATES) do
                w[#w + 1] = string.format("%08x=%d", va, mem.read_u32(va))
            end
            log("title: mode=0x%02x sub=0x%02x | %s", m, sub, table.concat(w, " "))
        end
        if m == 0x10 or m == 0x11 then
            -- Attract / "PRESS START": only START leaves it.
            mash(pad.BTN.START)
        elseif m == TITLE_MENU_MODE then
            -- Scripted, not steered. No word in the title state struct
            -- was found to track the menu row (the documented cursor
            -- stays 0, and the one candidate that moved turned out to be
            -- a free-running mod-3 counter), so the menu is driven the
            -- way a player would: the cursor starts on NEW GAME, so N
            -- separated DOWN taps put it on row N, then one CROSS
            -- confirms. Taps are spaced far enough apart to register as
            -- distinct presses.
            menu_ticks = menu_ticks + 1
            -- Settle first: taps sent while the menu is still animating
            -- in are swallowed, and the confirm then lands on row 0.
            if menu_ticks < MENU_SETTLE then
                return
            end
            local mt = menu_ticks - MENU_SETTLE
            local tap_span = MENU_ROW * MENU_TAP
            if mt < tap_span then
                if mt % MENU_TAP == 1 then
                    pad.force(pad.BTN.DOWN)
                elseif mt % MENU_TAP == 8 then
                    pad.release(pad.BTN.DOWN)
                end
            elseif mt < tap_span + MENU_TAP then
                pad.release(pad.BTN.DOWN)
                if mt == tap_span + 6 then
                    pad.force(pad.BTN.CROSS)
                elseif mt == tap_span + 14 then
                    pad.release(pad.BTN.CROSS)
                end
            else
                browse()
            end
        else
            -- Transitions and the save browser.
            browse()
        end
        if m == 3 and scene() ~= "" then set_phase("FIELD") end
        if phase_ticks > 12000 then
            log("never reached a field scene - dumping for diagnosis")
            dump()
        end
    elseif phase == "FIELD" then
        pad.release(pad.BTN.CROSS)
        if phase_ticks == 1 then
            -- "opdeene" here means the title path fell through to NEW GAME
            -- and this capture is NOT the save that was asked for.
            log("field scene '%s'%s", scene(),
                scene() == "opdeene" and "  <-- NEW GAME, not the card save!" or "")
        end
        -- Let the field settle, then force the fight rather than waiting
        -- on the encounter RNG.
        if phase_ticks > 120 then
            if #formation > 0 then
                for i = 1, 4 do
                    mem.write_u8(FORMATION + i - 1, formation[i] or 0)
                end
                log("forcing formation %s", FORMATION_S)
                mem.write_u16(GM, 8)
            end
            set_phase("BATTLE")
        end
    elseif phase == "BATTLE" then
        if m == 0x14 or m == 0x15 then
            battle_ticks = battle_ticks + 1
            if battle_ticks >= SETTLE then dump() end
        elseif phase_ticks > 4000 then
            log("battle never armed (mode 0x%02x) - dumping for diagnosis", m)
            dump()
        end
    end
end

os.execute("mkdir -p '" .. OUT_DIR .. "'")
log("armed: out=%s formation=%s settle=%d", OUT_DIR, FORMATION_S, SETTLE)
bp.arm(TITLE_BP, "Exec", 4, "title_tick", tick)
bp.arm(FIELD_BP, "Exec", 4, "field_tick", tick)
bp.arm(BATTLE_BP, "Exec", 4, "battle_draw", tick)
