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
-- Scenario: requires a save state with the game in STABLE FIELD MODE (not
-- mid-scene-transition). Library saves that still have a pending boot sequence
-- will reinitialize the SC block after our RAM fill, reverting the inventory.
-- Use a sstate that was saved from a fully-loaded field scene.
--
-- The script fills all 72 consumable slots with Water Talisman + sets gold to
-- 999,999, then attempts to trigger a full-bag ADD via two paths:
--   Path A: CROSS at the casino prize-exchange counter (koin1 context)
--   Path B: START + navigate to Equip → un-equip a weapon (EquipSwapBackRefund
--           caller FUN_8020E748, works from any field scene)
--
-- Run:
--   LEGAIA_SSTATE=~/Tools/pcsx-redux/SCUS94254.sstate2 \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_inventory_oob_writer.lua --frames 3600

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")

-- ── addresses ──────────────────────────────────────────────────────────────
local OOB_TARGET       = 0x800859E8   -- first key-item slot; OOB id write lands here
local WATCH_LEN        = 0x10         -- cover a few key-item slots; width-robust
local ADD_ID_STORE_PC  = 0x800422BC   -- FUN_800421D4 unguarded id store
local ITEM_WINDOW_BASE = 0x80085958   -- SC+0x1818, 2-byte (id, qty) stride
local ITEM_WINDOW_SLOTS = 72
local GOLD_ADDR        = 0x8008459C   -- u32 LE party gold (cheat-pinned)

local FILL_ID  = 0x73   -- Water Talisman
local FILL_QTY = 99

-- ── output ─────────────────────────────────────────────────────────────────
local OUT = probe.out_path("inventory_oob_writer.txt")
local f   = assert(io.open(OUT, "w"))

local g        = 0
local armed    = false
local handle   = nil
local oob_hits = 0
local seen     = {}

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 3600),
    snapshot_path  = OUT:gsub("%.txt$", ".hits.txt"),

    on_arm = function() return {} end,

    on_capture = function(ctx, elapsed)
        g = elapsed

        -- ── Phase 1 (vsync 5): fill inventory + boost gold ─────────────────
        if elapsed == 5 then
            local ok = 0
            for i = 0, ITEM_WINDOW_SLOTS - 1 do
                local a = ITEM_WINDOW_BASE + i * 2
                mem.write_u8(a,     FILL_ID)
                mem.write_u8(a + 1, FILL_QTY)
                if (mem.read_u8(a) or 0) == FILL_ID then ok = ok + 1 end
            end
            -- gold: write u32 LE = 999999 = 0x000F423F
            mem.write_u8(GOLD_ADDR,     0x3F)
            mem.write_u8(GOLD_ADDR + 1, 0x42)
            mem.write_u8(GOLD_ADDR + 2, 0x0F)
            mem.write_u8(GOLD_ADDR + 3, 0x00)
            local gold = (mem.read_u8(GOLD_ADDR + 3) or 0) * 0x1000000
                       + (mem.read_u8(GOLD_ADDR + 2) or 0) * 0x10000
                       + (mem.read_u8(GOLD_ADDR + 1) or 0) * 0x100
                       + (mem.read_u8(GOLD_ADDR)     or 0)
            f:write(string.format("vsync5: filled %d/%d slots  gold=%d\n", ok, ITEM_WINDOW_SLOTS, gold))
            f:flush()
        end

        -- ── Phase 2 (vsync 8): arm the write-watch ─────────────────────────
        if not armed and elapsed == 8 then
            f:write(string.format(
                "watch=[0x%08X,+0x%X)  add_id_store_pc=0x%08X\n",
                OOB_TARGET, WATCH_LEN, ADD_ID_STORE_PC))
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
                        "f=%-5d pc=0x%08X %s%s  t0=%02X a0=%08X\n",
                        g, rg.pc, rg.note,
                        is_oob and "  <== OOB ID STORE (FUN_800421D4)" or "",
                        (rg.t0 or 0) % 0x100, rg.a0 or 0))
                    f:flush()
                end,
            })
            armed = true
        end

        if not armed then return end

        -- ── Path A: casino prize-exchange (koin1 context) ──────────────────
        -- Press CROSS repeatedly early on to interact with the counter NPC.
        -- If the save state was made at the prize-exchange counter, the first
        -- CROSS opens the exchange. Subsequent presses confirm the purchase.
        -- Exchange requires coins (separate bank at 0x800845A4, not gold).
        if elapsed == 15 then
            -- also boost casino coins to 999999
            mem.write_u8(0x800845A4,     0xFF)
            mem.write_u8(0x800845A5,     0xE0)
            mem.write_u8(0x800845A6,     0xF5)
            mem.write_u8(0x800845A7,     0x05)
        end
        -- CROSS presses for prize-exchange interaction + confirm
        if elapsed == 20 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 22 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 30 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 32 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 40 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 42 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 50 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 52 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 60 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 62 then probe.pad_release(probe.BTN.CROSS) end

        -- ── Path B: START menu → Equip → un-equip (works from any scene) ───
        -- Press START to open field menu, then navigate to Equip and remove
        -- a weapon. EquipSwapBackRefund caller FUN_8020E748 fires the OOB
        -- when it tries to return the weapon to the full inventory.
        -- Timing: start late enough that Path A confirmations would have run.
        if elapsed == 90 then probe.pad_force(probe.BTN.START) end
        if elapsed == 92 then probe.pad_release(probe.BTN.START) end
        -- wait for menu to open, navigate to second option (Equip or Status)
        if elapsed == 105 then probe.pad_force(probe.BTN.DOWN) end
        if elapsed == 107 then probe.pad_release(probe.BTN.DOWN) end
        if elapsed == 115 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 117 then probe.pad_release(probe.BTN.CROSS) end
        -- select first character
        if elapsed == 130 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 132 then probe.pad_release(probe.BTN.CROSS) end
        -- navigate to first equip slot (weapon)
        if elapsed == 145 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 147 then probe.pad_release(probe.BTN.CROSS) end
        -- confirm unequip / press CROSS on "Remove" option
        if elapsed == 160 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 162 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 175 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 177 then probe.pad_release(probe.BTN.CROSS) end

        -- ── Path B retry: try again with different navigation offsets ───────
        if elapsed == 200 then probe.pad_force(probe.BTN.START) end
        if elapsed == 202 then probe.pad_release(probe.BTN.START) end
        if elapsed == 215 then probe.pad_force(probe.BTN.CROSS) end  -- first option
        if elapsed == 217 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 230 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 232 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 245 then probe.pad_force(probe.BTN.DOWN) end
        if elapsed == 247 then probe.pad_release(probe.BTN.DOWN) end
        if elapsed == 255 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 257 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 270 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 272 then probe.pad_release(probe.BTN.CROSS) end

        -- ── Path A retry: walk toward counter + CROSS ───────────────────────
        if elapsed == 310 then probe.pad_force(probe.BTN.UP) end
        if elapsed == 340 then probe.pad_release(probe.BTN.UP) end
        if elapsed == 355 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 357 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 365 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 367 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 375 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 377 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 385 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 387 then probe.pad_release(probe.BTN.CROSS) end

        -- walk RIGHT + CROSS
        if elapsed == 420 then probe.pad_force(probe.BTN.RIGHT) end
        if elapsed == 450 then probe.pad_release(probe.BTN.RIGHT) end
        if elapsed == 460 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 462 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 470 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 472 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 480 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 482 then probe.pad_release(probe.BTN.CROSS) end

        -- walk DOWN + CROSS
        if elapsed == 520 then probe.pad_force(probe.BTN.DOWN) end
        if elapsed == 550 then probe.pad_release(probe.BTN.DOWN) end
        if elapsed == 560 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 562 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 570 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 572 then probe.pad_release(probe.BTN.CROSS) end

        -- walk LEFT + CROSS
        if elapsed == 610 then probe.pad_force(probe.BTN.LEFT) end
        if elapsed == 640 then probe.pad_release(probe.BTN.LEFT) end
        if elapsed == 650 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 652 then probe.pad_release(probe.BTN.CROSS) end
        if elapsed == 660 then probe.pad_force(probe.BTN.CROSS) end
        if elapsed == 662 then probe.pad_release(probe.BTN.CROSS) end
    end,

    on_done = function()
        if handle then
            handle:dump(OUT:gsub("%.txt$", ".records.txt"))
        end
        local total = handle and handle:count() or 0
        f:write(string.format(
            "done; %d total stores to watched range, %d from the OOB id-store pc\n",
            total, oob_hits))
        if oob_hits > 0 then
            f:write("RESULT: OOB id store CONFIRMED reachable in normal play.\n")
            f:write(string.format("  pc=0x%08X  target=0x%08X\n",
                ADD_ID_STORE_PC, OOB_TARGET))
        else
            f:write("RESULT: no OOB id store observed this run.\n")
            f:write("  Bag may not have been full at add time, or add was not triggered.\n")
            f:write("  Try with --frames 7200 or use a sstate positioned at an open shop.\n")
        end
        f:close()
    end,
})
