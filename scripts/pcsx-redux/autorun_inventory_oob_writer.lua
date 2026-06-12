-- autorun_inventory_oob_writer.lua
--
-- ACE backlog 2.1 reachability probe. Confirms (or refutes) that normal play
-- can reach the unchecked inventory-add helper FUN_800421D4 with a FULL
-- consumable window, firing its out-of-bounds id store.
--
-- Background (docs/reference/functions.md `800421D4`, memory-map.md):
--   The add helper's id store `sb t0,0x1818(a0)` @ 0x800422BC is unconditional
--   and PRECEDES the bound check that guards only the count store. On a full
--   72-slot window the free-slot scan returns slot == window, so the id byte
--   lands one slot PAST the window at
--     ITEM_WINDOW_BASE + 72*2 = 0x80085958 + 0x90 = 0x800859E8
--   (= SC+0x18A8 = the first KEY-ITEM slot). The clean-room model of this is
--   `legaia_save::retail_inventory` (AddOutcome::OobIdWrite); this probe is the
--   live half that confirms the store actually executes in retail.
--
-- Method: arm probe.step.find_writer over [0x800859E8, +0x10) (width-correct;
-- catches the store regardless of width/alignment) and record every store with
-- its faulting PC + live registers + the post-store bytes. The DECISIVE signal
-- is a write to 0x800859E8 whose pc == 0x800422BC (the add helper's id store):
-- that is the OOB primitive firing. A write from any other pc is a normal
-- key-item update and is logged for disambiguation.
--
-- Scenario: needs a save state with a FULL consumable bag (all 72 slots
-- occupied) positioned to trigger an add. Any of the known unchecked callers
-- works (docs `800421D4` caller list): the easiest to drive by hand is the
-- shop buy-confirm (FUN_801C36B0) — stand at a shop and buy one more
-- consumable. Battle-loot drops (FUN_8004E568) and captured-monster pay
-- (FUN_801F138C) also reach it. Drive the buy/encounter manually during the
-- capture window, or extend the on_capture input block below for your save.
--
-- Run (supply your own full-bag scenario / sstate):
--   timeout --kill-after=30s 600s bash scripts/pcsx-redux/run_probe.sh \
--     --scenario inventory_full_shop \
--     --lua scripts/pcsx-redux/autorun_inventory_oob_writer.lua --frames 1200
--
-- Or with an explicit save state:
--   LEGAIA_SSTATE=~/Tools/pcsx-redux/full_bag_shop.sstate \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_inventory_oob_writer.lua --frames 1200

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

-- Fixed OOB target for the default 72-slot consumable window.
local OOB_TARGET = 0x800859E8
local WATCH_LEN = 0x10 -- cover a few key-item slots; width-robust
local ADD_ID_STORE_PC = 0x800422BC -- FUN_800421D4 unguarded id store
local ADD_HELPER_PC = 0x800421D4 -- helper entry (for context)

local OUT = probe.out_path("inventory_oob_writer.txt")
local f = assert(io.open(OUT, "w"))

local g = 0
local armed = false
local handle = nil
local oob_hits = 0
local seen = {}

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 1200),
    snapshot_path = OUT:gsub("%.txt$", ".hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g = elapsed
        if not armed then
            if elapsed >= 2 then
                f:write(string.format("watch=[0x%08X,+0x%X) add_id_store_pc=0x%08X\n",
                    OOB_TARGET, WATCH_LEN, ADD_ID_STORE_PC))
                f:write("drive a full-bag ADD now (shop buy / loot / capture)\n")
                f:flush()
                handle = probe.step.find_writer(OOB_TARGET, WATCH_LEN, {
                    read_len = WATCH_LEN,
                    on_write = function(rg)
                        local key = string.format("%08X:%s", rg.pc, rg.note)
                        if seen[key] then return end
                        seen[key] = true
                        local is_oob = (rg.pc == ADD_ID_STORE_PC)
                        if is_oob then oob_hits = oob_hits + 1 end
                        f:write(string.format(
                            "f=%-5d pc=0x%08X %s%s  t0=%02X a0=%08X v0=%08X\n",
                            g, rg.pc, rg.note,
                            is_oob and "  <== OOB ID STORE (FUN_800421D4)" or "",
                            -- t0 holds the id being stored at the id-store PC;
                            -- a0 = window base arg. Mask to a byte for the id.
                            (rg.t0 or 0) % 0x100, rg.a0 or 0, rg.v0 or 0))
                        f:flush()
                    end,
                })
                armed = true
            end
            return
        end
        -- Optional scripted input: confirm a shop purchase. Adjust to your save
        -- (shop cursor layout varies). Left here as a no-op-friendly default;
        -- manual driving during the window is the reliable path.
        -- if elapsed == 5 then probe.pad_force(probe.BTN.CROSS) end
        -- if elapsed == 7 then probe.pad_release(probe.BTN.CROSS) end
    end,
    on_done = function()
        if handle then
            handle:dump(OUT:gsub("%.txt$", ".records.txt"))
        end
        f:write(string.format(
            "done; %d total stores to watched range, %d from the OOB id-store pc\n",
            handle and handle:count() or 0, oob_hits))
        if oob_hits > 0 then
            f:write("RESULT: OOB id store CONFIRMED reachable in normal play.\n")
        else
            f:write("RESULT: no OOB id store observed this run "
                .. "(bag not full at the add? caller pre-checked room? "
                .. "add not triggered? — re-run with a confirmed full-bag save).\n")
        end
        f:close()
    end,
})
