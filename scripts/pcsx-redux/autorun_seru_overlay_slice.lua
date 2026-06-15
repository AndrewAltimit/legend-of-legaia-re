-- autorun_seru_overlay_slice.lua
--
-- HOOK-INDEPENDENT validation of the retail custom-overlay LOAD path
-- (legaia_rando::seru_overlay): proves on the emulator that the game's own
-- synchronous CD reader streams a hand-written overlay out of an overwritten
-- pochi PROT slot and executes it. Independent of any shop/battle trigger.
--
-- Requires the PATCHED disc (apply::inject_overlay_slice). Pass it as --iso.
--
-- Method: drive the call from the debugger, but execute ONLY code that arrives
-- via the emulated CD DMA (cache-coherent), never debugger-written gap code
-- (PCSX-Redux runs a stale decoded copy of debugger writes). We chain the
-- return: call FUN_8005E4D4 with ra = DEST, so the loader returns straight into
-- the freshly-loaded overlay. The overlay is a leaf ending `jr ra`; since ra is
-- still DEST it self-loops at DEST, re-writing the sentinel. We detect the run
-- with an Exec BP on the overlay's own `jr ra` (DEST+0x10), read the sentinel,
-- and quit.
--
-- Run:
--   timeout --kill-after=30s 300s bash scripts/pcsx-redux/run_probe.sh \
--     --iso /tmp/legaia_slice.bin \
--     --lua scripts/pcsx-redux/autorun_seru_overlay_slice.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 600)
local LBA      = probe.getenv_num("LEGAIA_SLICE_LBA", 28353)
local SECTORS  = probe.getenv_num("LEGAIA_SLICE_SECTORS", 1)

local LOADER_FN     = 0x8005E4D4
local DEST          = 0x801F69D8
local DEST_JR       = DEST + 0x10 -- the overlay's `jr ra` (word 4 of 6)
local SENTINEL_ADDR = 0x8007AF20
local SENTINEL      = 0x5E2D7ADE
local OVERLAY_W0    = 0x3C025E2D  -- lui v0,0x5E2D (overlay first word)
local DEST_MARKER   = 0xAAAAAAAA

local function w32(addr, v)
    probe.write_u16(addr, v % 0x10000)
    probe.write_u16(addr + 2, math.floor(v / 0x10000) % 0x10000)
end

local st = { phase = 0, entered = false, overlay_ran = false,
             dest0 = nil, got = nil }

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function()
        w32(SENTINEL_ADDR, 0)   -- clear (data write; fine)
        w32(DEST, DEST_MARKER)  -- poison so a real load is visible
        probe.arm_breakpoint(LOADER_FN, "Exec", 4, "loader_entry", function()
            if not st.entered then
                st.entered = true
                PCSX.log("[slice] FUN_8005E4D4 ENTERED")
            end
        end)
        -- The overlay's own jr ra: fires after the loaded code ran its store.
        probe.arm_breakpoint(DEST_JR, "Exec", 4, "overlay_jr", function()
            if not st.overlay_ran then
                st.overlay_ran = true
                st.dest0 = probe.read_u32(DEST)
                st.got = probe.read_u32(SENTINEL_ADDR)
                PCSX.log(string.format(
                    "[slice] overlay ran: DEST[0]=%08X sentinel=%08X", st.dest0, st.got))
            end
        end)
        PCSX.log(string.format(
            "[slice] armed: loader=%08X lba=%d sectors=%d dest=%08X",
            LOADER_FN, LBA, SECTORS, DEST))
        return {}
    end,
    on_capture = function(ctx, elapsed)
        if st.phase == 0 and elapsed >= 2 then
            -- Call FUN_8005E4D4(sectors, lba, dest) with ra=DEST: the loader
            -- returns straight into the freshly-loaded overlay.
            local r = PCSX.getRegisters()
            r.pc = LOADER_FN
            r.GPR.n.ra = DEST
            r.GPR.n.a0 = SECTORS
            r.GPR.n.a1 = LBA
            r.GPR.n.a2 = DEST
            st.phase = 1
            PCSX.log("[slice] phase1: called FUN_8005E4D4(ra=DEST) -> load + run")
        elseif st.phase == 1 and st.overlay_ran then
            st.phase = 2
            ctx.request_quit = true
        end
    end,
    on_done = function()
        local load_ok = st.dest0 == OVERLAY_W0
        local sentinel_ok = st.got == SENTINEL
        PCSX.log("=== seru overlay slice ===")
        PCSX.log(string.format("  loader entered: %s", tostring(st.entered)))
        PCSX.log(string.format("  overlay ran:    %s", tostring(st.overlay_ran)))
        PCSX.log(string.format("  load (DEST[0]): %s (%s)",
            tostring(load_ok), st.dest0 and string.format("%08X", st.dest0) or "nil"))
        PCSX.log(string.format("  sentinel:       %s (%s)",
            tostring(sentinel_ok), st.got and string.format("%08X", st.got) or "nil"))
        PCSX.log(string.format("  RESULT:         %s",
            (load_ok and sentinel_ok) and "PASS" or "FAIL"))
        PCSX.log("=== end ===")
    end,
})
