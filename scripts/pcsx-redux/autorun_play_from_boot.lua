-- autorun_play_from_boot.lua
--
-- Boot-onward scripted driver for the trace-driven-coverage program. Drives the
-- opening with pad injection - skip logos / "PRESS START" / intro FMV, confirm
-- NEW GAME (row 0), advance opening dialogue - by mashing START+CROSS, while
-- logging the game_mode timeline. When it reaches a target mode it writes a
-- createSaveState checkpoint, which the host gzips + catalogs (manage-states.py)
-- into the immutable save library as a reproducible, fingerprinted segment
-- anchor. See docs/tooling/playthrough-coverage.md.
--
-- This is a BESPOKE GPU::Vsync listener, NOT probe.run: it polls game_mode from
-- the very first frame and reacts to state transitions, so it is not gated on a
-- boot_delay vsync count (which never accumulates during the pre-render CD-boot
-- phase). Navigation is pure pad injection - no breakpoints.
--
-- COLD BOOT (validated): launch with `-interpreter -debugger -fastboot`. The
-- title's XA-BGM streaming stops VSync(0) delivery to this autorun (and it does
-- not resume through the field load), so the vsync listener cannot drive. Two
-- per-frame EXEC BREAKPOINTS - which fire on CPU execution regardless of GPU
-- rendering - drive everything instead (see make_tick):
--   * FUN_801DD35C (TITLE_BP) - the title tick; fires through the title +
--     field-INIT; mashes START+CROSS (PRESS-START gate + NEW GAME confirm).
--   * FUN_8001698C (FIELD_BP) - the default mode handler's per-frame vsync-sync;
--     fires at field-RUN + 12-13 of 14 game modes (where the title tick stops);
--     mashes CROSS only (in-game advance) + does target-detect + checkpoint.
-- Validated end to end: cold boot -> title -> NEW GAME -> opening prologue
-- (scene "opdeene") field-RUN -> checkpoint (S1); then resume S1 + CROSS-mash
-- through the prologue scenes -> Rim Elm "town01" -> checkpoint (S2). Use a
-- SETTLE >= ~20 so the checkpoint lands at field-RUN (stable/resumable), not the
-- title tick's brief field-INIT window (which segfaults on resume).
--
-- Env vars:
--   LEGAIA_SSTATE      resume from this save (segment chaining); cold boot if
--                      empty or LEGAIA_NO_SSTATE=1.
--   LEGAIA_CKPT_MODE   target game_mode to checkpoint at (default 3 = field-run).
--   LEGAIA_CKPT_SCENE  target active scene name (e.g. town01); overrides CKPT_MODE.
--   LEGAIA_CKPT_LABEL  checkpoint file stem (default "s1_field").
--   LEGAIA_TICK_BP     title-tick exec-bp addr (default 0x801DD35C; 0 disables).
--   LEGAIA_FIELD_BP    field-tick exec-bp addr (default 0x8001698C; 0 disables).
--   LEGAIA_OUT_DIR     output dir (default captures/play).
--   LEGAIA_MASH_EVERY  frames between button pulses (default 20).
--   LEGAIA_SETTLE      frames at the target before checkpointing (default 30).
--   LEGAIA_MAX_FRAMES  safety cap.
--   LEGAIA_NO_MASH     "1" = observe only (log the timeline, arm no tick BPs).
--
-- Output:
--   <OUT_DIR>/play.log              the game_mode transition timeline
--   <OUT_DIR>/<LABEL>.rawsstate     raw (uncompressed) createSaveState protobuf;
--                                   host: `gzip -c x.rawsstate > x.sstate` then
--                                   `manage-states.py backup pcsx-redux x.sstate`

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local GM         = 0x8007B83C -- game_mode (low byte = mode value)
local TITLE_CD   = 0x801EF16C -- title attract countdown (init 0x8000, ticks down)
local SCENE_NAME = 0x8007050C -- active scene-name buffer ("opdeene", "town01", ...)
-- Optional: checkpoint when the active scene name matches this (segment chaining
-- by scene, e.g. CKPT_SCENE=town01 to stop at Rim Elm). Empty = use CKPT_MODE.
local CKPT_SCENE = env.getenv("LEGAIA_CKPT_SCENE", "")
if CKPT_SCENE == "" then CKPT_SCENE = nil end
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/play")
local CKPT_MODE  = tonumber(env.getenv("LEGAIA_CKPT_MODE", "3")) or 3
local CKPT_LABEL = env.getenv("LEGAIA_CKPT_LABEL", "s1_field")
local MASH_EVERY = tonumber(env.getenv("LEGAIA_MASH_EVERY", "20")) or 20
local SETTLE     = tonumber(env.getenv("LEGAIA_SETTLE", "30")) or 30
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "20000")) or 20000
local NO_MASH    = env.getenv("LEGAIA_NO_MASH", "") == "1"
-- Mash START (skip logos + "PRESS START" gate) AND CROSS (confirm NEW GAME row 0
-- + advance opening dialogue). Pressing both each pulse covers every screen.
local MASH_BTNS  = { pad.BTN.START, pad.BTN.CROSS }
local function mash_press()   for _, b in ipairs(MASH_BTNS) do pad.force(b) end end
local function mash_release() for _, b in ipairs(MASH_BTNS) do pad.release(b) end end
-- Non-vsync tick: an exec breakpoint on the per-frame title tick FUN_801DD35C
-- fires once per title frame REGARDLESS of GPU::Vsync (which the title's XA-BGM
-- streaming stops delivering to Lua). Driving the mash from it lets us confirm
-- NEW GAME at the title before it idles to the attract FMV. Needs the
-- interpreter (`-interpreter -debugger`). 0 / "" disables. Default = the title
-- tick; harmless in-game (the address isn't executed once past the title).
-- Two per-frame tick breakpoints (see make_tick): the title tick and the field
-- vsync-sync. 0 / "" disables either.
local TITLE_BP   = tonumber(env.getenv("LEGAIA_TICK_BP", "0x801DD35C")) or 0
local FIELD_BP   = tonumber(env.getenv("LEGAIA_FIELD_BP", "0x8001698C")) or 0
-- Optional start save: resume from a catalogued checkpoint to drive the NEXT
-- segment forward (segment chaining). Empty / LEGAIA_NO_SSTATE=1 => cold boot.
local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local NO_SSTATE  = env.getenv("LEGAIA_NO_SSTATE", "") == "1"
local START_DELAY = tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/play.log", "w")
local function log(s)
    PCSX.log("[play] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end

local function read_mode()
    if not mem.in_ram(GM) then return nil end
    return mem.read_u8(GM)
end
local function read_cd()
    if not mem.in_ram(TITLE_CD) then return nil end
    return mem.read_u32(TITLE_CD)
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
-- Target reached? scene-name match if CKPT_SCENE is set, else game_mode match.
local function reached_target()
    if CKPT_SCENE then return read_scene() == CKPT_SCENE end
    return read_mode() == CKPT_MODE
end

-- Write the full createSaveState slice to disk. PCSX.createSaveState() returns
-- a wrapper `{_type="Slice", _wrapper=cdata}`; this build does NOT export the
-- ffi slice accessors (getSliceSize/getSliceData), so it is written through the
-- Support.File API: `Support.File.open(path,"CREATE"):writeMoveSlice(slice)`,
-- which emits the raw (uncompressed) protobuf. The host then gzips it into the
-- GUI .sstate format (gzip(protobuf)) - byte-validated to round-trip + load.
local function do_write_checkpoint()
    local w = PCSX.createSaveState()
    if w == nil then log("createSaveState returned nil"); return false end
    if Support == nil or Support.File == nil then log("no Support.File API"); return false end
    local path = OUT_DIR .. "/" .. CKPT_LABEL .. ".rawsstate"
    local fh = Support.File.open(path, "CREATE")
    if fh == nil or (fh.failed and fh:failed()) then log("cannot open " .. path); return false end
    fh:writeMoveSlice(w)
    fh:close()
    local sf = io.open(path, "rb")
    local sz = sf and sf:seek("end") or 0
    if sf then sf:close() end
    if sz <= 1024 then log("checkpoint too small (" .. sz .. " bytes)"); return false end
    log(string.format("checkpoint: %s (%d bytes raw; host-gzip to .sstate)", path, sz))
    return true
end

local function write_checkpoint()
    log("write_checkpoint: capturing...")
    local ok, err = pcall(do_write_checkpoint)
    if not ok then log("write_checkpoint error: " .. tostring(err)) end
    return ok
end

local PHASE = "ADVANCE" -- ADVANCE -> DONE (driven by the tick BPs)
local vsync = 0
local last_mode = -1
local start_loaded = false

local function on_vsync()
    vsync = vsync + 1
    -- Optional: resume from a start save (segment chaining). Cold boot when
    -- LEGAIA_NO_SSTATE=1 or no LEGAIA_SSTATE given.
    if not start_loaded and not NO_SSTATE and START_SAVE ~= ""
        and vsync >= START_DELAY then
        start_loaded = true
        if sstate.load(START_SAVE) then
            log("resumed from start save " .. START_SAVE)
        else
            log("FAILED to load start save " .. START_SAVE)
        end
    end
    local m = read_mode()
    if m ~= nil and m ~= last_mode then
        local cd = read_cd()
        log(string.format("vsync %d: mode 0x%02X -> 0x%02X (title_cd=%s)",
            vsync, last_mode < 0 and 0xFF or last_mode, m,
            cd and string.format("0x%X", cd) or "n/a"))
        last_mode = m
    end
    -- heartbeat so a stuck phase is visible without a mode change
    if (vsync % 180) == 0 then
        local cd = read_cd()
        log(string.format("...vsync %d phase=%s mode=0x%02X title_cd=%s",
            vsync, PHASE, m or 0xFF, cd and string.format("0x%X", cd) or "n/a"))
    end

    -- All input + target-detection + checkpoint is driven by the per-frame exec
    -- breakpoints (see make_tick below): GPU::Vsync delivery to Lua stops during
    -- XA streaming and does not resume through the field load, so it cannot be
    -- the driver. on_vsync only loads the start save + logs the timeline.
end

-- Unified per-frame tick. Exec breakpoints fire on CPU execution regardless of
-- GPU::Vsync delivery, so they drive the whole opening even while the vsync
-- listener is blind. TWO ticks cover every phase:
--   * the title tick FUN_801DD35C (TITLE_BP) - fires through the title +
--     field-INIT, mashes START+CROSS (PRESS-START gate + NEW GAME confirm);
--   * the per-frame vsync-sync FUN_8001698C (FIELD_BP) - the default mode
--     handler's tick, fires at field-RUN + 12-13 of 14 game modes where the
--     title tick stops, mashes CROSS only (in-game advance; START opens the
--     field menu).
-- Both feed one shared SM (counter / target-detect / checkpoint). With a SETTLE
-- larger than the title tick's brief field-INIT window, the checkpoint lands at
-- field-RUN (via FIELD_BP) - a stable, resumable state.
local g_tick = 0
local g_mash_until = 0
local g_target_since = nil
local g_quit_at = nil

local function make_tick(press_fn, release_fn, label)
    return function()
        g_tick = g_tick + 1
        if PHASE == "DONE" then
            if g_quit_at and g_tick >= g_quit_at then
                if LOG then LOG:close() end
                PCSX.quit(0)
            end
            return
        end
        if PHASE ~= "ADVANCE" or NO_MASH then return end
        if (g_tick % 60) == 0 then
            log(string.format("[%s tick %d] mode=0x%02X scene=%q",
                label, g_tick, read_mode() or 0xFF, read_scene()))
        end
        if g_mash_until > 0 and g_tick >= g_mash_until then
            release_fn(); g_mash_until = 0
        elseif (g_tick % MASH_EVERY) == 0 and g_mash_until == 0 then
            press_fn(); g_mash_until = g_tick + 5
        end
        if reached_target() then
            if g_target_since == nil then
                g_target_since = g_tick
            elseif g_tick - g_target_since >= SETTLE then
                release_fn()
                log(string.format("settled at target (mode 0x%02X scene=%q, tick %d); checkpointing",
                    read_mode() or 0xFF, read_scene(), g_tick))
                write_checkpoint()
                PHASE = "DONE"
                g_quit_at = g_tick + 2
            end
        else
            g_target_since = nil
        end
    end
end

local function cross_press() pad.force(pad.BTN.CROSS) end
local function cross_release() pad.release(pad.BTN.CROSS) end

if not NO_MASH then
    local armed = {}
    if TITLE_BP ~= 0 then
        pcall(function() bp.arm(TITLE_BP, "Exec", 4, "title_tick",
            make_tick(mash_press, mash_release, "title")) end)
        armed[#armed + 1] = string.format("title=0x%08X", TITLE_BP)
    end
    if FIELD_BP ~= 0 then
        pcall(function() bp.arm(FIELD_BP, "Exec", 4, "field_tick",
            make_tick(cross_press, cross_release, "field")) end)
        armed[#armed + 1] = string.format("field=0x%08X", FIELD_BP)
    end
    log("tick BPs armed: " .. table.concat(armed, " "))
end

log(string.format("driver: target mode=0x%02X scene=%s label=%s out=%s",
    CKPT_MODE, tostring(CKPT_SCENE), CKPT_LABEL, OUT_DIR))
PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
