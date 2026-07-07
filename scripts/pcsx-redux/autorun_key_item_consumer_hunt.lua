-- autorun_key_item_consumer_hunt.lua
--
-- ACE Phase 3 / Path C: identify native code that reads the key-item area
-- (0x800859E8..0x80085A40) to find an unsafe consumer that could turn the
-- OOB id-store primitive into a debug-warp enable.
--
-- Background:
--   The full-bag add helper (FUN_800421D4) writes the added item's id byte to
--   0x800859E8, then 0x800859EA, etc. (2-byte slot stride, count guarded).
--   These addresses sit in the key-item list following the 72-slot consumable
--   window. If any native code later reads one of those bytes and uses it as an
--   index / pointer / length without a bound check, we may be able to reach the
--   debug bytes 0x8007B8C2 / 0x8007B98F via a chain.
--
-- Method:
--   1. Load a stable field save state (casino area works; any mode accepted).
--   2. RAM-fill the 72-slot consumable window so the next add fires OOB.
--   3. Optionally seed key-item slot 0 with a chosen id (LEGAIA_TRIGGER_OOB=1).
--   4. Arm Read BPs on the first WATCH_LEN bytes of the key-item area.
--   5. Log every read: frame, address, value, pc, ra, mode.
--   6. Also arm Write BPs on the two debug bytes (passive listeners only).
--
-- After the run, grep for unique PCs in the log to find consumers. The
-- heartbeat prints a sorted unique-PC summary for easy post-analysis.
--
-- Run:
--   LEGAIA_SSTATE=~/Tools/pcsx-redux/SCUS94254.sstate2 \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_key_item_consumer_hunt.lua
--
-- Set LEGAIA_TRIGGER_OOB=1 to seed key-item slot 0 with LEGAIA_OOB_ID (default
-- 0x9C, the casino prize id confirmed by autorun_inventory_oob_writer.lua).
--
-- Output: captures/key_item_consumer_hunt/<ts>/key_item_consumer_hunt.txt

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local bp    = require("probe.bp")

-- ── addresses ──────────────────────────────────────────────────────────────
local ITEM_WINDOW_BASE  = 0x80085958   -- 72-slot consumable window
local ITEM_WINDOW_SLOTS = 72
local KEY_ITEM_START    = 0x800859E8   -- first OOB id-store target
local KEY_ITEM_END      = 0x80085A40   -- end of item inventory array
local GAME_MODE         = 0x8007B83C
local DEBUG_B8C2        = 0x8007B8C2   -- retail-vs-dev asset-load selector
local DEBUG_B98F        = 0x8007B98F   -- debug menu enable

-- ── knobs ──────────────────────────────────────────────────────────────────
local WATCH_LEN    = 0x18   -- first 24 bytes of key-item area (12 slots × 2 bytes)
local FILL_ID      = 0x73   -- Water Talisman (harmless fill id)
local TRIGGER_OOB  = (probe.getenv("LEGAIA_TRIGGER_OOB", "0"):lower() ~= "0")
local OOB_ID       = tonumber(probe.getenv("LEGAIA_OOB_ID", "0x9C")) or 0x9C
-- Soft cap on logged read events to prevent flooding the output file.
local MAX_LOG      = 3000

-- ── output ─────────────────────────────────────────────────────────────────
local OUT = probe.out_path("key_item_consumer_hunt.txt")
local f   = assert(io.open(OUT, "w"))

local function logline(s)
    f:write(s .. "\n"); f:flush()
    PCSX.log("[ki-hunt] " .. s)
end

local function u8(addr)  return mem.read_u8(addr)  or 0 end
local function u16(addr) return mem.read_u16(addr) or 0 end

-- ── state ──────────────────────────────────────────────────────────────────
local vsync        = 0
local loaded_at    = nil
local filled       = false
local triggered    = false
local armed        = false
local read_count   = 0
local seen_pcs     = {}   -- pc_str -> { count, first_addr }

-- ── helpers ────────────────────────────────────────────────────────────────
local function fill_consumable_window()
    for i = 0, ITEM_WINDOW_SLOTS - 1 do
        local a = ITEM_WINDOW_BASE + i * 2
        mem.write_u8(a,     FILL_ID)
        mem.write_u8(a + 1, 99)
    end
    local ok = 0
    for i = 0, ITEM_WINDOW_SLOTS - 1 do
        if u8(ITEM_WINDOW_BASE + i * 2) == FILL_ID then ok = ok + 1 end
    end
    return ok
end

-- Seed the first key-item id byte directly. This simulates having already
-- triggered the OOB store via a shop/casino buy with a full bag, so the probe
-- can focus on who READS those bytes rather than needing to navigate menus.
local function seed_key_item_slot(id)
    mem.write_u8(KEY_ITEM_START, id)
    logline(string.format("seeded key-item slot 0 with id=0x%02X @ 0x%08X",
        id, KEY_ITEM_START))
end

local function pc_key(pc_num)
    return string.format("0x%08X", bit.band(pc_num, 0xFFFFFFFF))
end

local function print_unique_pc_summary()
    local keys = {}
    for k, _ in pairs(seen_pcs) do keys[#keys + 1] = k end
    table.sort(keys)
    logline(string.format("=== unique PCs that read the key-item area (%d total reads) ===",
        read_count))
    for _, k in ipairs(keys) do
        local e = seen_pcs[k]
        logline(string.format("  pc=%s  hits=%-4d  first_addr=0x%08X",
            k, e.count, e.first_addr))
    end
    logline(string.format("=== end summary (%d unique PCs) ===", #keys))
end

-- ── vsync handler ─────────────────────────────────────────────────────────
local function on_vsync()
    vsync = vsync + 1

    if loaded_at == nil then
        if vsync >= 60 then
            if not probe.load_save_state(probe.getenv("LEGAIA_SSTATE",
                os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2")) then
                logline("FATAL: could not load save state")
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

    -- Wait 5 vsyncs for RAM to settle, then fill. No mode check: the sstate
    -- can be in any game mode (casino = 0x17, field = 0x03, etc.).
    if not filled and since >= 5 then
        local ok = fill_consumable_window()
        logline(string.format("filled %d/%d consumable slots at vsync %d; mode=0x%04X",
            ok, ITEM_WINDOW_SLOTS, vsync, u16(GAME_MODE)))
        filled = true
    end
    if not filled then return end

    if TRIGGER_OOB and not triggered and since >= 8 then
        seed_key_item_slot(OOB_ID)
        triggered = true
    end

    -- Arm read watchpoints + passive debug-byte write listeners once.
    if not armed and since >= 10 then
        armed = true

        -- Read BPs on the key-item window.
        for off = 0, WATCH_LEN - 1 do
            local addr = KEY_ITEM_START + off
            bp.arm(addr, "Read", 1,
                string.format("ki_read_%02X", off),
                function()
                    local r  = PCSX.getRegisters()
                    local pc = tonumber(r.pc) or 0
                    local ra = tonumber(r.GPR.n.ra) or 0
                    read_count = read_count + 1
                    local k = pc_key(pc)
                    if not seen_pcs[k] then
                        seen_pcs[k] = { count = 0, first_addr = addr }
                    end
                    seen_pcs[k].count = seen_pcs[k].count + 1
                    if read_count <= MAX_LOG then
                        f:write(string.format(
                            "READ  f=%-5d addr=0x%08X val=0x%02X pc=0x%08X ra=0x%08X mode=0x%04X\n",
                            vsync, addr, u8(addr),
                            bit.band(pc, 0xFFFFFFFF),
                            bit.band(ra, 0xFFFFFFFF),
                            u16(GAME_MODE)))
                        if read_count == MAX_LOG then
                            f:write("(read log cap reached; unique-PC tracking continues)\n")
                            f:flush()
                        end
                    end
                end)
        end

        -- Passive write listeners on the debug bytes.
        bp.arm(DEBUG_B8C2, "Write", 1, "dbg_b8c2_write", function()
            local r  = PCSX.getRegisters()
            local pc = bit.band(tonumber(r.pc) or 0, 0xFFFFFFFF)
            logline(string.format(
                "WRITE to 0x8007B8C2 pc=0x%08X val=0x%02X (loader flag!)",
                pc, u8(DEBUG_B8C2)))
        end)
        bp.arm(DEBUG_B98F, "Write", 1, "dbg_b98f_write", function()
            local r  = PCSX.getRegisters()
            local pc = bit.band(tonumber(r.pc) or 0, 0xFFFFFFFF)
            logline(string.format(
                "WRITE to 0x8007B98F pc=0x%08X val=0x%02X (debug menu flag!)",
                pc, u8(DEBUG_B98F)))
        end)

        logline(string.format(
            "armed %d Read BPs on [0x%08X, +0x%X); passive Write BPs on 0x%08X + 0x%08X",
            WATCH_LEN, KEY_ITEM_START, WATCH_LEN, DEBUG_B8C2, DEBUG_B98F))
        logline("READY - navigate menus / trigger item adds / warp scenes to generate reads.")
        logline("grep the output for unique PCs; heartbeat prints a summary every ~8 s.")
    end

    -- Heartbeat every ~8 s: also flush read log and print unique-PC summary.
    if armed and (since % 480) == 0 and since > 0 then
        f:flush()
        logline(string.format("alive f=%d mode=0x%04X key-item-reads=%d",
            vsync, u16(GAME_MODE), read_count))
        print_unique_pc_summary()
    end
end

-- ── startup ────────────────────────────────────────────────────────────────
logline("=== autorun_key_item_consumer_hunt ===")
logline(string.format("key-item range:  0x%08X..0x%08X", KEY_ITEM_START, KEY_ITEM_END))
logline(string.format("watch window:    [0x%08X, +0x%X)  (%d bytes = %d key-item slots)",
    KEY_ITEM_START, WATCH_LEN, WATCH_LEN, WATCH_LEN / 2))
logline(string.format("debug targets:   0x%08X (loader)  0x%08X (menu)",
    DEBUG_B8C2, DEBUG_B98F))
logline(string.format("TRIGGER_OOB=%s OOB_ID=0x%02X  MAX_LOG=%d",
    TRIGGER_OOB and "yes" or "no", OOB_ID, MAX_LOG))
logline("")
logline("This probe logs every native read of the key-item bytes (the OOB write target).")
logline("After the run, check the summary for unique PCs - those are the consumers.")
logline("Any consumer that uses the id byte as an index without a bound is an exploit chain.")
logline("")

-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
logline("vsync listener installed; waiting for boot")
