-- autorun_spine_flag_writers.lua
--
-- Chapter-1 story-spine flag-writer hunt. Arms ALL THREE spine writes at
-- once so a single interactive play-forward session can bracket every one
-- of them AND name the caller PC (ra) that performs the write:
--
--   1. DAT_8007b7fc = 0x4B  (Zeto battle-id write, Mt. Rikuroa trigger)
--        raw Write-watch on 0x8007b7fc, width 1 (widen to 4 via
--        LEGAIA_ZETO_WIDTH if the byte watch never fires).
--   2. system flag 0x142    (dolk-dungeon clear)
--        Exec-bp on the flag setter FUN_8003CE08 (0x8003CE08), filtered
--        on a0 == 322; logs the caller ra. This isolates the exact flag
--        AND names the writer directly, which a raw byte watch can't do
--        (eight flags share one bank byte).
--   3. system flag 0x482    (Drake mist walls)
--        Exec-bp on 0x8003CE08 filtered on a0 == 1154; logs ra.
--
-- Flag-bank geometry (SCUS_942.54-resident): base = 0x80085758,
--   byte = base + (idx >> 3), mask = 0x80 >> (idx & 7).
--   flag 0x142 (322)  -> byte 0x80085780, mask 0x20
--   flag 0x482 (1154) -> byte 0x800857E8, mask 0x20
-- The exec-bp path is preferred; if a flag write is missed, fall back to a
-- raw Write-watch on the bank byte (set LEGAIA_FLAG_FALLBACK=1) which fires
-- for any of the eight flags in that byte and needs the ra decoded by hand.
--
-- This is a HUMAN-NAVIGATED session, not a scripted capture: it installs a
-- bare Vsync listener with NO self-quit (mirrors autorun_flag_bank_watcher).
-- Load a chapter-1 card save, play forward through the beats, and watch the
-- CSV. Wrap the launch in `timeout --kill-after` (the probe never exits on
-- its own). Full beat order + card-save guidance:
--   docs/tooling/spine-flag-writers-capture.md
--
-- Launch (MUST be -interpreter -debugger; Lua BPs are dead under --fast):
--   LEGAIA_NO_SSTATE=1 \
--   xvfb-run -a timeout --kill-after=15s 1800s \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_spine_flag_writers.lua
--
--   (or LEGAIA_SSTATE=/path/to/state.sstate to seed from a save-state
--    instead of a card save.)
--
-- Output:
--   spine_flag_writers.csv        tick,label,addr,pc,ra,value
--   spine_flag_writers.detail.txt call-context for the first N hits/label
--
-- A "caught" hit is a CSV row whose ra column is NON-ZERO (the writer's
-- return address). For the Zeto row, value should read 0x4B (75). For the
-- flag rows, value is the flag index (322 / 1154) and ra is the caller.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local bp    = require("probe.bp")
local bit   = require("bit")

-- +-- addresses -------------------------------------------------------------
local GAME_MODE      = 0x8007B83C  -- u8; field mode = 0x03
local ZETO_ADDR      = 0x8007B7FC  -- DAT_8007b7fc: battle-id staging byte
local FLAG_SET_PC    = 0x8003CE08  -- FUN_8003CE08: set bit; a0 = flag index
local FLAG_BANK_BASE = 0x80085758  -- byte array base

-- Target flag indices and their expected bank byte / mask (for the log).
local FLAG_DOLK_CLEAR = 322   -- 0x142
local FLAG_MIST_WALLS = 1154  -- 0x482

-- +-- config ----------------------------------------------------------------
local SSTATE     = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local NO_SSTATE  = probe.getenv("LEGAIA_NO_SSTATE", "") == "1"
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local ZETO_WIDTH = probe.getenv_num("LEGAIA_ZETO_WIDTH", 1)   -- 1, widen to 4 if silent
local MAX_DETAIL = probe.getenv_num("LEGAIA_MAX_DETAIL", 8)   -- per-label call-context dumps
local FALLBACK   = probe.getenv("LEGAIA_FLAG_FALLBACK", "") == "1"
local ARM_STABLE = 6  -- consecutive field-mode frames before arming exec-bps

local CSV = probe.csv_open(probe.out_path("spine_flag_writers.csv"),
    "tick,label,addr,pc,ra,value")
local DETAIL = probe.out_path("spine_flag_writers.detail.txt")

-- +-- helpers ----------------------------------------------------------------
local function u8(addr)  return mem.read_u8(addr)  or 0 end
local function u16(addr) return mem.read_u16(addr) or 0 end
local function regs()    return PCSX.getRegisters() end

-- Bank byte + mask for a flag index (mirrors FUN_8003CE08's addressing).
local function flag_byte(idx)
    return FLAG_BANK_BASE + math.floor(idx / 8), bit.rshift(0x80, idx % 8)
end

-- +-- state ------------------------------------------------------------------
local vsync        = 0
local loaded_at    = nil
local armed        = false
local field_frames = 0
local hit_totals   = {}  -- label -> total hits
local detail_hits  = {}  -- label -> call-context dumps emitted so far

local function log(s)
    CSV.fh:flush()
    PCSX.log("[spine] " .. s)
end

-- Record one hit: CSV row (tick,label,addr,pc,ra,value) + first-N call context.
local function record(label, addr, pc, ra, value)
    CSV:row("%d,%s,0x%08X,0x%08X,0x%08X,%d", vsync, label, addr, pc, ra, value)
    hit_totals[label] = (hit_totals[label] or 0) + 1
    PCSX.log(string.format(
        "[spine] HIT %-22s tick=%d addr=0x%08X pc=0x%08X ra=0x%08X value=%d",
        label, vsync, addr, pc, ra, value))
    local n = (detail_hits[label] or 0)
    if n < MAX_DETAIL then
        detail_hits[label] = n + 1
        probe.append_call_context(DETAIL, probe.capture_call_context(
            string.format("%s hit #%d tick=%d addr=0x%08X value=%d",
                label, n + 1, vsync, addr, value)))
    end
end

-- +-- arm the three watches --------------------------------------------------
local function arm_all()
    -- 1) Zeto battle-id: raw Write-watch on the staging byte.
    local reader = (ZETO_WIDTH == 4) and mem.read_u32 or mem.read_u8
    bp.arm(ZETO_ADDR, "Write", ZETO_WIDTH, "zeto_battle_id", function()
        local r  = regs()
        local pc = bit.band(tonumber(r.pc), 0xFFFFFFFF)
        local ra = bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF)
        record("zeto_battle_id", ZETO_ADDR, pc, ra, reader(ZETO_ADDR) or 0)
    end)

    -- 2 + 3) Flag setter, filtered on the two target indices. The setter is
    -- hot, so the callback checks a0 first and returns immediately for every
    -- flag that is not one of the two spine flags.
    bp.arm(FLAG_SET_PC, "Exec", 4, "flag_setter", function()
        local r  = regs()
        local a0 = tonumber(r.GPR.n.a0) or 0
        local idx = a0 % 0x10000
        local label
        if idx == FLAG_DOLK_CLEAR then
            label = "flag_0x142_dolk_clear"
        elseif idx == FLAG_MIST_WALLS then
            label = "flag_0x482_mist_walls"
        else
            return
        end
        local ra = bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF)
        local baddr = (flag_byte(idx))
        record(label, baddr, FLAG_SET_PC, ra, idx)
    end)

    -- Optional fallback: raw Write-watch on the two bank bytes. Fires for any
    -- of the eight flags in the byte; the ra must be disambiguated by hand.
    if FALLBACK then
        local dolk_byte = (flag_byte(FLAG_DOLK_CLEAR))
        local mist_byte = (flag_byte(FLAG_MIST_WALLS))
        bp.arm(dolk_byte, "Write", 1, "flag_0x142_byte_fallback", function()
            local r  = regs()
            local pc = bit.band(tonumber(r.pc), 0xFFFFFFFF)
            local ra = bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF)
            record("flag_0x142_byte_fallback", dolk_byte, pc, ra, u8(dolk_byte))
        end)
        bp.arm(mist_byte, "Write", 1, "flag_0x482_byte_fallback", function()
            local r  = regs()
            local pc = bit.band(tonumber(r.pc), 0xFFFFFFFF)
            local ra = bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF)
            record("flag_0x482_byte_fallback", mist_byte, pc, ra, u8(mist_byte))
        end)
    end

    armed = true
    local dolk_byte, dolk_mask = flag_byte(FLAG_DOLK_CLEAR)
    local mist_byte, mist_mask = flag_byte(FLAG_MIST_WALLS)
    log(string.format("armed at tick %d (mode=0x%02X)", vsync, u8(GAME_MODE)))
    log(string.format("  zeto_battle_id : Write-watch 0x%08X width %d",
        ZETO_ADDR, ZETO_WIDTH))
    log(string.format("  flag 0x142     : Exec-bp 0x%08X a0==%d -> byte 0x%08X mask 0x%02X",
        FLAG_SET_PC, FLAG_DOLK_CLEAR, dolk_byte, dolk_mask))
    log(string.format("  flag 0x482     : Exec-bp 0x%08X a0==%d -> byte 0x%08X mask 0x%02X",
        FLAG_SET_PC, FLAG_MIST_WALLS, mist_byte, mist_mask))
    log(FALLBACK and "  fallback byte watches ENABLED" or "  fallback byte watches off")
    log("play the spine: keikoku -> rikuroa (Zeto) -> victory -> dolk clear -> mist walls")
end

-- +-- vsync loop -------------------------------------------------------------
local function on_vsync()
    vsync = vsync + 1

    -- Optionally seed from a save-state (skipped for card-save play).
    if loaded_at == nil then
        if NO_SSTATE then
            loaded_at = vsync
            log("LEGAIA_NO_SSTATE=1 -- load a chapter-1 card save by hand")
        elseif vsync >= BOOT_DELAY then
            if not probe.load_save_state(SSTATE) then
                log("FATAL: could not load save state; check LEGAIA_SSTATE (or set LEGAIA_NO_SSTATE=1)")
                loaded_at = -1
                return
            end
            loaded_at = vsync
            log(string.format("state loaded at tick %d; mode=0x%02X", vsync, u8(GAME_MODE)))
        end
        return
    end
    if loaded_at < 0 then return end

    -- Arm once the game has settled into field mode (0x03). Arming the exec-bp
    -- while still in a menu/battle-load blip can crash PCSX-Redux, so require a
    -- few consecutive field-mode frames first.
    if not armed then
        if u8(GAME_MODE) == 0x03 then
            field_frames = field_frames + 1
            if field_frames >= ARM_STABLE then arm_all() end
        else
            field_frames = 0
        end
        return
    end

    -- Heartbeat every ~8s so a long human session shows the probe is alive.
    if (vsync % 480) == 0 then
        log(string.format("alive tick=%d mode=0x%02X zeto=%d dolk=%d mist=%d",
            vsync, u8(GAME_MODE),
            hit_totals["zeto_battle_id"] or 0,
            hit_totals["flag_0x142_dolk_clear"] or 0,
            hit_totals["flag_0x482_mist_walls"] or 0))
    end
end

-- +-- startup ----------------------------------------------------------------
log("=== autorun_spine_flag_writers ===")
log("purpose: catch the three chapter-1 spine story-flag writers in one session")
log(string.format("sstate=%s%s", NO_SSTATE and "(card play; NO_SSTATE)" or SSTATE,
    NO_SSTATE and "" or ""))
log("value column: zeto=byte at 0x8007b7fc (expect 0x4B/75); flags=flag index (322/1154)")
log("a caught write = a CSV row with a NON-ZERO ra")
log("this session never self-quits -- wrap the launch in timeout --kill-after")
log(string.format("mode/game_mode addr = 0x%08X (field = 0x03)", GAME_MODE))

PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
log("vsync listener installed; waiting for field mode to arm")
