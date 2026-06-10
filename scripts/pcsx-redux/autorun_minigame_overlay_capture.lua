-- autorun_minigame_overlay_capture.lua
--
-- Minigame-entry overlay-window capture. The OTHER game-mode pair (24/25)
-- hosts the minigames; the field VM's 0x3E opcode with operand >= 100
-- writes game_mode 0x18 (24) + operand-100 into the sub-id at 0x8007BA34.
-- PROT 0896 (bat_back_dat, self-consistent static base 0x801C5818) is
-- believed to be the mode-24 OTHER overlay, but it is NOT resident in any
-- parked mode-25 library state (each minigame's own overlay has replaced
-- it) nor in the pre-transition entry state (still mode 3). The only
-- window left is the mode-24 entry itself.
--
-- This probe polls game_mode (0x8007B83C) per vsync. The first vsync it
-- reads 0x18 OR 0x19 (in case init blinks past within one frame), it:
--   * logs the trigger vsync + sub-id (0x8007BA34) + overlay slot
--     pointers (0x8001038C / 0x80010390);
--   * dumps the overlay window 0x801C0000..0x80200000 at +0, +10 and
--     +30 vsyncs after the trigger;
--   * dumps full 2 MiB main RAM once after the +30 window dump, then
--     quits.
-- Offline, byte-match each dump against the PROT 0896 as-loaded payload
-- with scripts/pcsx-redux/overlay_residency.py.
--
-- Run while ENTERING a minigame: either load the pre-transition entry
-- save (scenario baka_fighter_entry_pretransition; the sit animation
-- rolls into the transition with no input), or run without a save state
-- and play into any minigame by hand.
--
-- Env vars:
--   LEGAIA_SSTATE     save state path (default: sstate4, the live
--                     pre-transition Baka Fighter entry slot)
--   LEGAIA_NO_SSTATE  if "1", skip the save-state load (play by hand)
--   LEGAIA_OUT_DIR    output dir (default: captures/minigame_overlay)
--   LEGAIA_FRAMES     max vsyncs to wait for the mode write (default 3600)
--
-- Output:
--   <OUT_DIR>/window_plus<N>.bin   overlay window at trigger+N vsyncs
--   <OUT_DIR>/ram_full.bin         full main RAM after the last window
--   <OUT_DIR>/summary.txt          trigger vsync, modes, sub-id, slot ptrs

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local MODE_ADDR   = 0x8007B83C
local SUBID_ADDR  = 0x8007BA34
local SLOT_A_PTR  = 0x8001038C
local SLOT_B_PTR  = 0x80010390
local WIN_LO      = 0x801C0000
local WIN_LEN     = 0x00040000 -- to end of main RAM

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate4")
local OUT_DIR     = probe.getenv("LEGAIA_OUT_DIR", "captures/minigame_overlay")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 3600)
local NO_SSTATE   = probe.getenv("LEGAIA_NO_SSTATE", "") == "1"

if NO_SSTATE then
    probe.load_save_state = function(_)
        PCSX.log("[minigame] LEGAIA_NO_SSTATE=1 -- play into a minigame by hand")
        return true
    end
end

os.execute(string.format("mkdir -p %q", OUT_DIR))

local DUMP_OFFSETS = { 0, 10, 30 }
local trigger_vsync = nil  -- capture-relative vsync of the first 0x18/0x19 read
local dumps_done    = 0
local summary       = {}

local function log_line(s)
    PCSX.log("[minigame] " .. s)
    summary[#summary + 1] = s
end

local function dump_window(tag)
    local buf = probe.read_bytes(WIN_LO, WIN_LEN)
    if buf == nil then
        log_line("FATAL: cannot read overlay window for " .. tag)
        return
    end
    local path = string.format("%s/window_%s.bin", OUT_DIR, tag)
    local fh = io.open(path, "wb")
    fh:write(tostring(buf))
    fh:close()
    log_line(string.format("dumped %s (0x%08X..0x%08X)", path, WIN_LO, WIN_LO + WIN_LEN))
end

local function log_globals(elapsed)
    log_line(string.format(
        "vsync=%d mode=0x%02X sub_id=0x%08X slotA=0x%08X slotB=0x%08X",
        elapsed,
        probe.read_u8(MODE_ADDR),
        probe.read_u32(SUBID_ADDR),
        probe.read_u32(SLOT_A_PTR),
        probe.read_u32(SLOT_B_PTR)))
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,

    on_arm = function(_)
        log_line(string.format("watching mode addr 0x%08X for 0x18/0x19; out=%s",
            MODE_ADDR, OUT_DIR))
        return {}
    end,

    on_capture = function(ctx, elapsed)
        if trigger_vsync == nil then
            local mode = probe.read_u8(MODE_ADDR)
            if mode == 0x18 or mode == 0x19 then
                trigger_vsync = elapsed
                log_line(string.format("TRIGGER mode=0x%02X at vsync %d", mode, elapsed))
                log_globals(elapsed)
            end
        end
        if trigger_vsync ~= nil and dumps_done < #DUMP_OFFSETS then
            local want = trigger_vsync + DUMP_OFFSETS[dumps_done + 1]
            if elapsed >= want then
                log_globals(elapsed)
                dump_window(string.format("plus%02d", DUMP_OFFSETS[dumps_done + 1]))
                dumps_done = dumps_done + 1
                if dumps_done >= #DUMP_OFFSETS then
                    ctx.request_quit = true
                end
            end
        end
    end,

    on_done = function(_, _)
        if trigger_vsync == nil then
            log_line("NO TRIGGER: game_mode never read 0x18/0x19 within the frame budget")
        else
            local buf = probe.read_bytes(0x80000000, probe.RAM_SIZE)
            if buf ~= nil then
                local fh = io.open(OUT_DIR .. "/ram_full.bin", "wb")
                fh:write(tostring(buf))
                fh:close()
                log_line("dumped full main RAM to " .. OUT_DIR .. "/ram_full.bin")
            end
        end
        local fh = io.open(OUT_DIR .. "/summary.txt", "w")
        fh:write(table.concat(summary, "\n") .. "\n")
        fh:close()
    end,
})
