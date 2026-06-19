-- autorun_inventory_fill.lua
--
-- One-shot GameShark-equivalent inventory fill for Legend of Legaia (NTSC-U).
-- Loads a save state, waits for RAM to settle, then writes a configurable set
-- of item-slot (id, qty) pairs directly into RAM - exactly what a batch of
-- GameShark `30` codes would do. PCSX-Redux is left running so you can
-- open the in-game save menu and write a real memory-card save from the
-- patched state.
--
-- GameShark `30` code recap (for cross-reference):
--   30XXYYYY 00ZZ  =  write byte ZZ to address 0x800XYYYY
--   Slot N id byte  = 0x80085958 + N*2      (e.g. slot 0 = 0x80085958)
--   Slot N qty byte = 0x80085958 + N*2 + 1  (e.g. slot 0 = 0x80085959)
--
-- Usage:
--   Edit SSTATE and SLOTS below, then:
--     bash scripts/pcsx-redux/run_probe.sh \
--       --lua scripts/pcsx-redux/autorun_inventory_fill.lua
--
--   Or with a named scenario:
--     bash scripts/pcsx-redux/run_probe.sh \
--       --scenario <your_scenario> \
--       --lua scripts/pcsx-redux/autorun_inventory_fill.lua
--
-- After the script prints "DONE - save in-game now", open the in-game menu,
-- go to a save point, and save normally. The script does NOT quit PCSX-Redux.
--
-- Item id reference: docs/reference/gamedata.md, or use
--   cargo run -p legaia-asset -- item-names   (if built)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local mem = require("probe.mem")
local sstate = require("probe.sstate")
local env = require("probe.env")

-- ============================================================
-- CONFIG - edit these two sections for your session
-- ============================================================

-- Save state to load. Defaults to your default pcsx-redux sstate.
-- Override with: LEGAIA_SSTATE=/path/to/file.sstate
local SSTATE_PATH = env.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")

-- Inventory slots to fill.
-- Each entry: { slot = 0-based slot index, id = item id byte, qty = count }
-- Slot indices 0..71 are the consumable window (ITEM_WINDOW_BASE 0x80085958).
-- IDs: see docs/reference/gamedata.md "Items" or the GameShark codes you have.
-- Example below: fill all 72 slots with Water Talisman (id 0x73, qty 99) -
-- this produces a full bag that triggers the OOB add primitive on any further
-- item add (ACE backlog 2.1 reachability capture).
--
-- To use a different layout, replace the block inside the loop or list entries
-- manually:
--   SLOTS = {
--       { slot = 0,  id = 0x73, qty = 99 },   -- Water Talisman x99
--       { slot = 1,  id = 0x01, qty = 5  },   -- Healing Leaf   x5
--   }
local SLOTS = (function()
    local t = {}
    for i = 0, 71 do
        t[#t + 1] = { slot = i, id = 0x73, qty = 99 }  -- Water Talisman x99
    end
    return t
end)()

-- ============================================================
-- Constants (match crates/save/src/retail_inventory.rs)
-- ============================================================
local ITEM_WINDOW_BASE = 0x80085958   -- SC+0x1818, 2-byte (id,qty) stride
local ITEM_WINDOW_SLOTS = 72

-- ============================================================
-- Write loop - runs once after RAM settles
-- ============================================================

local function slot_addr(slot)
    return ITEM_WINDOW_BASE + slot * 2
end

local function do_fill()
    local ok_count = 0
    local err_count = 0
    local log_lines = {}

    for _, entry in ipairs(SLOTS) do
        local s = entry.slot
        if s < 0 or s >= ITEM_WINDOW_SLOTS then
            PCSX.log(string.format("[inventory_fill] SKIP slot %d: out of range [0,%d)", s, ITEM_WINDOW_SLOTS))
        else
            local id_addr  = slot_addr(s)
            local qty_addr = id_addr + 1
            local id_val   = bit.band(entry.id,  0xFF)
            local qty_val  = bit.band(entry.qty, 0xFF)

            -- Read before
            local id_before  = mem.read_u8(id_addr)  or 0
            local qty_before = mem.read_u8(qty_addr) or 0

            -- Write (GameShark 30 equivalent)
            local id_ok  = mem.write_u8(id_addr,  id_val)
            local qty_ok = mem.write_u8(qty_addr, qty_val)

            -- Read back to confirm
            local id_after  = mem.read_u8(id_addr)  or 0
            local qty_after = mem.read_u8(qty_addr) or 0

            if id_ok and qty_ok and id_after == id_val and qty_after == qty_val then
                ok_count = ok_count + 1
                -- Only log the first few and last slot to keep output tidy
                if s < 3 or s == ITEM_WINDOW_SLOTS - 1 then
                    log_lines[#log_lines + 1] = string.format(
                        "  slot %02d  [0x%08X] id: 0x%02X->0x%02X  qty: %d->%d",
                        s, id_addr, id_before, id_after, qty_before, qty_after)
                elseif s == 3 then
                    log_lines[#log_lines + 1] = string.format(
                        "  ... (%d more slots) ...", ITEM_WINDOW_SLOTS - 4)
                end
            else
                err_count = err_count + 1
                PCSX.log(string.format(
                    "[inventory_fill] ERROR slot %d: write_ok=%s/%s readback id=0x%02X(want 0x%02X) qty=%d(want %d)",
                    s, tostring(id_ok), tostring(qty_ok), id_after, id_val, qty_after, qty_val))
            end
        end
    end

    PCSX.log(string.format("[inventory_fill] %d slots written, %d errors", ok_count, err_count))
    for _, line in ipairs(log_lines) do PCSX.log(line) end

    -- Verify the OOB target byte is what it was before (we didn't touch it).
    local oob_target = ITEM_WINDOW_BASE + ITEM_WINDOW_SLOTS * 2  -- 0x800859E8
    local oob_val = mem.read_u8(oob_target)
    PCSX.log(string.format(
        "[inventory_fill] OOB target 0x%08X = 0x%02X (first key-item id; should be unchanged)",
        oob_target, oob_val or 0))

    PCSX.log("[inventory_fill] DONE - open the in-game save menu and save now.")
    PCSX.log("[inventory_fill] Script is detached; PCSX-Redux continues running.")
end

-- ============================================================
-- VSync driver - load sstate, wait 3 frames, write once, detach
-- ============================================================

local vsync = 0
local done  = false
local listener

local function on_vsync()
    vsync = vsync + 1
    if done then return end

    if vsync == 1 then
        -- Load the save state on the first VSync after the script starts.
        PCSX.log("[inventory_fill] loading save state: " .. SSTATE_PATH)
        local ok, err = pcall(sstate.load, SSTATE_PATH)
        if not ok then
            PCSX.log("[inventory_fill] ERROR loading sstate: " .. tostring(err))
            done = true
        end
        return
    end

    if vsync >= 4 then
        -- RAM has settled (3 frames post-load). Write slots and detach.
        done = true
        do_fill()
        -- Detach the listener so we stop consuming VSync callbacks.
        if listener then
            pcall(function() listener:disconnect() end)
        end
    end
end

listener = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
PCSX.log("[inventory_fill] vsync listener installed; will write after RAM settles")
