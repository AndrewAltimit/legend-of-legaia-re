-- autorun_flag_bank_watcher.lua
--
-- ACE Phase 3 reconnaissance: identify which fourth-flag-bank flag indices
-- in the OOB-REACHABLE range (5248..32767, byte offsets 0x290..0xFFF from
-- the flag bank base at 0x80085758) get SET by the game during normal play
-- and during the credits/ending sequence.
--
-- Background:
--   The OOB id-store primitive (FUN_800421D4 @ 0x800422BC) writes one byte
--   to 0x800859E8 on the first full-bag casino buy, then 0x800859EA on the
--   second, etc. (2-byte slot stride). 0x800859E8 = fourth-flag-bank byte
--   offset 0x290 = flag indices 5248..5255. Each additional buy covers 2
--   more bytes = 16 more flag indices.
--
--   The debug flag (0x8007B98F) is ~0x87B1 bytes BELOW the SC block start
--   (0x80084140) — OOB fills UPWARD, so it is unreachable via forward fill.
--   The flag bank overlap (flags 5248..32767) is the only live game-logic
--   surface in the OOB range.
--
-- Method: exec-breakpoint on FUN_8003CE08 (flag SET) and FUN_8003CE34 (flag
-- CLEAR). When called, a0 = flag index (u16). Log all calls; flag to the
-- console whenever idx >= 5248 (OOB-reachable range).
--
-- What to look for in the log:
--   Any "OOB-REACHABLE" line during or just before the credits sequence means
--   that flag, when set, is part of the game's ending logic. If the same flag
--   is CHECKED (test op = FUN_8003CE64) as a GATE (if flag then skip), we
--   need to SET it first to skip past the gate.
--
-- MINIMUM BUY CALCULATION:
--   Flag index N:
--     byte_addr = 0x80085758 + (N >> 3)
--     buys needed = ((byte_addr - 0x800859E8) / 2) + 1 if byte_addr >= 0x800859E8
--     prize id: must have bit (0x80 >> (N & 7)) set
--
-- Instructions (human-in-the-loop):
--   1. Wait for the save state to load.
--   2. Press SELECT + TRIANGLE to open the debug menu.
--   3. Navigate to MAP-CHANGE / WARP and select the credits/ending.
--   4. Watch the log for "OOB-REACHABLE" lines.
--   5. Close PCSX when done.
--
-- Run:
--   LEGAIA_SSTATE=~/Tools/pcsx-redux/SCUS94254.sstate2 \
--   LEGAIA_DEBUG_POKE=menu \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_flag_bank_watcher.lua
--
-- Output: captures/flag_bank_watcher/<ts>/flag_bank_watcher.txt

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local bp    = require("probe.bp")

-- ── addresses ──────────────────────────────────────────────────────────────
local DEBUG_MENU_HI  = 0x8007B98F
local DEBUG_WORD     = 0x8007B98C
local GAME_MODE      = 0x8007B83C

-- Flag bank ops (SCUS_942.54-resident, confirmed in the dumped disassembly).
local FLAG_SET_PC    = 0x8003CE08  -- FUN_8003CE08: set bit at (idx>>3), mask=0x80>>(idx&7). a0=idx
local FLAG_CLR_PC    = 0x8003CE34  -- FUN_8003CE34: clear bit.  a0=idx
local FLAG_TST_PC    = 0x8003CE64  -- FUN_8003CE64: test bit.   a0=idx (read-only; logged for context)

local bit            = require("bit")
local FLAG_BANK_BASE = 0x80085758  -- base address of the byte array
local OOB_START      = 0x800859E8  -- = FLAG_BANK_BASE + 0x290 = first OOB-reachable byte

-- First OOB-reachable flag index.
-- OOB_START - FLAG_BANK_BASE = 0x290; byte_off * 8 = first idx in that byte.
local OOB_MIN_IDX    = (OOB_START - FLAG_BANK_BASE) * 8  -- 5248

-- ── probe setup ────────────────────────────────────────────────────────────
local SSTATE     = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2")
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local SETTLE     = 5

local OUT = probe.out_path("flag_bank_watcher.txt")
local f   = assert(io.open(OUT, "w"))

local function logline(s)
    f:write(s .. "\n"); f:flush()
    PCSX.log("[flag-watch] " .. s)
end

local function u8(addr)  return mem.read_u8(addr)  or 0 end
local function u16(addr) return mem.read_u16(addr) or 0 end
local function u32(addr) return mem.read_u32(addr) or 0 end
local function regs()    return PCSX.getRegisters() end

-- ── state ──────────────────────────────────────────────────────────────────
local vsync        = 0
local loaded_at    = nil
local poked        = false
local armed        = false
local revert_count = 0
local last_word    = nil
local set_count    = 0
local clr_count    = 0
local tst_count    = 0

-- ── flag-bank call decoder ────────────────────────────────────────────────
-- `idx` is the raw a0 value when the flag op fires.
-- Return: flag_idx (u16, treating a0 as signed for the reachable-range check),
--         byte_addr, bit_position, is_oob_reachable, buys_needed.
local function decode_flag_call(idx_raw)
    -- The engine treats idx as i16 (signed); we care about the unsigned OOB
    -- range so keep both.
    local idx_u = idx_raw % 0x10000
    local byte_off = math.floor(idx_u / 8)
    local bit_pos  = idx_u % 8  -- 0 = MSB (mask 0x80), 7 = LSB (mask 0x01)
    local byte_addr = FLAG_BANK_BASE + byte_off
    local mask = bit.rshift(0x80, bit_pos)  -- bit pattern that must be set in prize id
    local is_oob = (byte_addr >= OOB_START)
    local buys = -1
    if is_oob then
        -- Each casino buy advances 2 bytes (2-byte slot stride in the add scan).
        buys = math.floor((byte_addr - OOB_START) / 2) + 1
    end
    return idx_u, byte_addr, bit_pos, mask, is_oob, buys
end

-- ── vsync handler ─────────────────────────────────────────────────────────
local function on_vsync()
    vsync = vsync + 1

    -- Step 1: wait for emulator to settle, then load the sstate.
    if loaded_at == nil then
        if vsync >= BOOT_DELAY then
            if not probe.load_save_state(SSTATE) then
                logline("FATAL: could not load save state; check LEGAIA_SSTATE path")
                loaded_at = -1
                return
            end
            loaded_at = vsync
            logline(string.format("state loaded at vsync %d; mode=0x%04X",
                vsync, u16(GAME_MODE)))
        end
        return
    end
    if loaded_at < 0 then return end

    local since = vsync - loaded_at

    -- Step 2: poke debug menu once the scene has settled.
    if not poked and since >= SETTLE then
        mem.write_u8(DEBUG_MENU_HI, 1)
        poked = true
        local ok = (u8(DEBUG_MENU_HI) == 1)
        logline(string.format(
            "debug_menu_hi poked at vsync %d; readback=%s  word@B98C=0x%08X",
            vsync, ok and "OK" or "MISMATCH", u32(DEBUG_WORD)))
        last_word = u32(DEBUG_WORD)
    end
    if not poked then return end

    -- Re-assert every vsync (scene-init can revert it).
    if u8(DEBUG_MENU_HI) ~= 1 then
        revert_count = revert_count + 1
        mem.write_u8(DEBUG_MENU_HI, 1)
    end

    -- Step 3: arm exec-breakpoints on the flag SET/CLEAR/TEST functions.
    if poked and not armed and since >= (SETTLE + 1) then
        armed = true

        -- ── FLAG SET (FUN_8003CE08) ─────────────────────────────────────
        bp.arm(FLAG_SET_PC, "Exec", 4, "flag_set", function()
            local r = regs()
            local idx_raw = tonumber(r.GPR.n.a0) or 0
            local idx, baddr, bpos, mask, is_oob, buys = decode_flag_call(idx_raw)
            set_count = set_count + 1
            local tag = is_oob and "  *** OOB-REACHABLE ***" or ""
            f:write(string.format(
                "SET  f=%-5d  idx=%-5d(0x%04X)  byte=0x%08X  bit=%d  mask=0x%02X  mode=0x%04X%s\n",
                vsync, idx, idx, baddr, bpos, mask, u16(GAME_MODE), tag))
            if is_oob then
                f:write(string.format(
                    "     OOB: buy #%d  prize id must have (id & 0x%02X) != 0\n",
                    buys, mask))
            end
            f:flush()
        end)

        -- ── FLAG CLEAR (FUN_8003CE34) ───────────────────────────────────
        bp.arm(FLAG_CLR_PC, "Exec", 4, "flag_clr", function()
            local r = regs()
            local idx_raw = tonumber(r.GPR.n.a0) or 0
            local idx, baddr, bpos, mask, is_oob, buys = decode_flag_call(idx_raw)
            clr_count = clr_count + 1
            local tag = is_oob and "  *** OOB-REACHABLE ***" or ""
            f:write(string.format(
                "CLR  f=%-5d  idx=%-5d(0x%04X)  byte=0x%08X  bit=%d  mask=0x%02X  mode=0x%04X%s\n",
                vsync, idx, idx, baddr, bpos, mask, u16(GAME_MODE), tag))
            f:flush()
        end)

        -- ── FLAG TEST (FUN_8003CE64) ────────────────────────────────────
        -- Log OOB-range tests only (to avoid noise from the frequent low-idx
        -- scene-entry checks).
        bp.arm(FLAG_TST_PC, "Exec", 4, "flag_tst", function()
            local r = regs()
            local idx_raw = tonumber(r.GPR.n.a0) or 0
            local idx, baddr, bpos, mask, is_oob, buys = decode_flag_call(idx_raw)
            if not is_oob then return end  -- skip noise from common low-index tests
            tst_count = tst_count + 1
            f:write(string.format(
                "TST  f=%-5d  idx=%-5d(0x%04X)  byte=0x%08X  bit=%d  mask=0x%02X  mode=0x%04X  *** OOB-REACHABLE ***\n",
                vsync, idx, idx, baddr, bpos, mask, u16(GAME_MODE)))
            f:write(string.format(
                "     OOB: buy #%d  prize id must have (id & 0x%02X) != 0\n",
                buys, mask))
            f:flush()
        end)

        logline(string.format(
            "armed: flag_SET @ 0x%08X  flag_CLR @ 0x%08X  flag_TST @ 0x%08X (OOB-range only)",
            FLAG_SET_PC, FLAG_CLR_PC, FLAG_TST_PC))
        logline("OOB-REACHABLE range: flag indices 5248..32767 (buy #1 at flag 5248..5255)")
        logline("")
        logline("READY. Press SELECT+TRIANGLE to open the debug menu, then warp to credits.")
    end

    -- Note master-word transitions.
    local w = u32(DEBUG_WORD)
    if last_word and w ~= last_word then
        logline(string.format("mode-word 0x%08X -> 0x%08X  (debug gate %s)",
            last_word, w, w ~= 0 and "ON" or "OFF"))
    end
    last_word = w

    -- Heartbeat every ~8s.
    if armed and (since % 480) == 0 and since > 0 then
        logline(string.format(
            "alive vsync=%d mode=0x%04X set=%d clr=%d tst_oob=%d reverts=%d",
            vsync, u16(GAME_MODE), set_count, clr_count, tst_count, revert_count))
    end
end

-- ── startup ────────────────────────────────────────────────────────────────
logline("=== autorun_flag_bank_watcher ===")
logline(string.format("sstate=%s", SSTATE))
logline("purpose: find which flag indices >= 5248 (OOB-reachable) get SET during credits/ending")
logline("")
logline("MEMORY MAP SUMMARY:")
logline("  SC block:       0x80084140..0x80086140 (8 KB)")
logline("  Fourth flag bank base: 0x80085758 (SC+0x1618)")
logline("  Consumable inv: 0x80085958..0x800859E7 (72 slots)")
logline("  OOB start:      0x800859E8 = flag bank byte 0x290 = indices 5248..5255")
logline("  Flag bank end:  0x80086757 = flag bank byte 0xFFF = indices 32760..32767")
logline("  Debug flag 0x8007B98F is BELOW SC base by 0x87B1 bytes -- NOT reachable via OOB")
logline("")
logline("INSTRUCTIONS:")
logline("  1. Wait for the save state to load (casino scene, field mode).")
logline("  2. Press SELECT + TRIANGLE to open the debug menu.")
logline("  3. Warp to the credits/ending scene via the map-change option.")
logline("  4. Watch for '*** OOB-REACHABLE ***' lines -- each is a flag the game")
logline("     sets during the ending that the OOB chain could trigger with N buys.")
logline("  5. Close PCSX when done.")
logline("")
logline("FLAG INTERPRETATION:")
logline("  SET line with OOB tag => game is setting flag idx during the ending.")
logline("  TST line with OOB tag => game is TESTING flag idx (a GATE check).")
logline("  For a gate: if the flag is not SET, the game blocks progress.")
logline("  Setting it via OOB (the right buy count + prize id) would skip that gate.")
logline("")
logline("MINIMUM BUYS FORMULA:")
logline("  flag idx N -> byte at 0x80085758 + (N>>3)")
logline("  buys = floor((byte_addr - 0x800859E8) / 2) + 1")
logline("  prize id: must have bit (0x80>>(N&7)) set; e.g. flag 5248 (bit0) needs prize id with bit 0x80 set")

PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
logline("vsync listener installed; waiting for boot")
