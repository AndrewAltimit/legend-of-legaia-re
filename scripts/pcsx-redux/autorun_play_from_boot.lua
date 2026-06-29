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
-- COLD BOOT requires the emulator's `-fastboot` flag (the default boot path
-- stalls on an early CD read in headless -run). With it the game boots fine and
-- reaches the title - but this listener goes vsync-BLIND there: the title's
-- XA-BGM streaming stops VSync(0) delivery to the autorun, so the vsync-gated
-- mash can't confirm NEW GAME and the title idles to the attract FMV (GDB shows
-- game_mode advancing to 0x1A while the listener sees a "freeze"). Navigating
-- the title needs a tick source that survives XA streaming (an exec-bp on the
-- per-frame title tick FUN_801DD35C, or host-driven input over the GDB stub).
-- The RESUME path (LEGAIA_SSTATE = a catalogued checkpoint) is fully working and
-- is how the segment corpus is grown: each run resumes the previous checkpoint
-- and drives forward through dense in-game vsyncs. See the doc.
--
-- Env vars:
--   LEGAIA_SSTATE      resume from this save (segment chaining); cold boot if
--                      empty or LEGAIA_NO_SSTATE=1.
--   LEGAIA_CKPT_MODE   target game_mode to checkpoint at (default 3 = field-run;
--                      2 = field-launch, 0x15 = battle, 0x17 = menu, 0x1A = STR).
--   LEGAIA_CKPT_LABEL  checkpoint file stem (default "s1_field").
--   LEGAIA_OUT_DIR     output dir (default captures/play).
--   LEGAIA_MASH_EVERY  vsyncs between START+CROSS pulses (default 20).
--   LEGAIA_SETTLE      vsyncs to hold at the target mode before checkpointing
--                      (default 30) so a transient pass-through isn't captured.
--   LEGAIA_MAX_FRAMES  safety cap; checkpoint wherever we are if exceeded.
--   LEGAIA_NO_MASH     "1" = observe only (log the mode timeline, no input).
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

local GM         = 0x8007B83C -- game_mode (low byte = mode value)
local TITLE_CD   = 0x801EF16C -- title attract countdown (init 0x8000, ticks down)
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/play")
local CKPT_MODE  = tonumber(env.getenv("LEGAIA_CKPT_MODE", "3")) or 3
local CKPT_LABEL = env.getenv("LEGAIA_CKPT_LABEL", "s1_field")
local MASH_EVERY = tonumber(env.getenv("LEGAIA_MASH_EVERY", "20")) or 20
local SETTLE     = tonumber(env.getenv("LEGAIA_SETTLE", "30")) or 30
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "20000")) or 20000
local NO_MASH    = env.getenv("LEGAIA_NO_MASH", "") == "1"
-- During XA-streamed intro/FMV phases the game stops calling VSync(0), so the
-- GPU::Vsync-driven mash can't pulse. HOLD_SKIP forces START+CROSS held
-- continuously (pad override persists without vsyncs), so a level-triggered
-- FMV/"PRESS START" skip sees the button down even while no frames render.
local HOLD_SKIP  = env.getenv("LEGAIA_HOLD_SKIP", "") == "1"
-- Mash START (skip logos + "PRESS START" gate) AND CROSS (confirm NEW GAME row 0
-- + advance opening dialogue). Pressing both each pulse covers every screen.
local MASH_BTNS  = { pad.BTN.START, pad.BTN.CROSS }
local function mash_press()   for _, b in ipairs(MASH_BTNS) do pad.force(b) end end
local function mash_release() for _, b in ipairs(MASH_BTNS) do pad.release(b) end end
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

local PHASE = "ADVANCE" -- ADVANCE -> AT_TARGET -> DONE
local vsync = 0
local last_mode = -1
local target_since = nil
local quit_at = nil
local mash_until = 0
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

    if PHASE == "ADVANCE" then
        if HOLD_SKIP and not NO_MASH then
            -- Force the buttons held once; the override persists through frozen
            -- (no-vsync) FMV/XA phases where pulsing can't fire.
            mash_press()
        else
            if mash_until > 0 and vsync >= mash_until then
                mash_release(); mash_until = 0
            end
            if not NO_MASH and (vsync % MASH_EVERY) == 0 and mash_until == 0 then
                mash_press(); mash_until = vsync + 5
            end
        end
        if m == CKPT_MODE then
            if target_since == nil then
                target_since = vsync
            elseif vsync - target_since >= SETTLE then
                mash_release()
                log(string.format("settled at target mode 0x%02X; checkpointing", CKPT_MODE))
                PHASE = "AT_TARGET"
            end
        else
            target_since = nil
        end
        if vsync >= MAX_FRAMES then
            mash_release()
            log(string.format("MAX_FRAMES at mode 0x%02X; checkpointing in place",
                m or 0xFF))
            PHASE = "AT_TARGET"
        end
    elseif PHASE == "AT_TARGET" then
        write_checkpoint()
        PHASE = "DONE"
        quit_at = vsync + 30
    elseif PHASE == "DONE" then
        if quit_at and vsync >= quit_at then
            if LOG then LOG:close() end
            PCSX.quit(0)
        end
    end
end

log(string.format("driver: target mode=0x%02X label=%s mash=%s out=%s",
    CKPT_MODE, CKPT_LABEL, tostring(not NO_MASH), OUT_DIR))
PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
