-- autorun_custom_items_e2e.lua
--
-- End-to-end battle test of the three custom items on a patched disc:
-- walk the world map into a random encounter, seed the bag with the items,
-- then force successive party commits to USE each one (a forced item
-- commit: +0x1DE=1, +0x1DF=<item>, +0x1DD=0 - the ally-slot target the
-- real item menu would commit):
--   1. Nature's Elixir (0xB9): expect target HP+MP jump to max, popup, no
--      wedge (the class-0x48 arm + retail HP tail).
--   2. Seru Tear (0x12): expect the conversion receipt (CONV1 fires,
--      +0x1DE flips to 2, spell = the caster's own Ra-Seru summon), the
--      MP-skip receipt (MPSKIP fires with the flag set, MP unchanged),
--      and the summon completing (battle continues afterward).
--   3. Fury Bloom (0x1A): expect the fury receipt (FURY ARM fires) and
--      every living party member's +0x1F9 gauge flag set.
--
-- The SCUS half (records / descriptors / JT words / caves) is RAM-installed
-- via LEGAIA_POKES; the battle-overlay hooks ride the patched --iso.
--
-- Launch:
--   LEGAIA_POKES="$(./target/debug/legaia-patcher delilas-pokes --custom-items)" \
--   LEGAIA_FRAMES=45000 \
--   bash scripts/pcsx-redux/run_probe.sh --iso <patched.bin> \
--     --scenario sol_to_karisto_worldmap \
--     --lua scripts/pcsx-redux/autorun_custom_items_e2e.lua
--
-- Output: custom_items_e2e.csv  tick,mode,note

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE   = 0x8007B83C
local ACTOR_TABLE = 0x801C9370
local BCTX_PTR    = 0x8007BD24
local BAG         = 0x80085958 -- 0x80084140 + 0x1818, 2 bytes/slot
local SEED_HOOK   = 0x801E2D60
local CONV1_VA    = 0x8003EDAC
local CONV2_VA    = 0x8003F210
local MPSKIP_VA   = 0x80042128 -- FURY_ARM_VA + 35*4 (packed after the fury arm)
local FURY_VA     = 0x8004209C
local ELIXIR_ARM  = 0x80025054
local FLAG_VA     = 0x80026100 -- DISPLAY_TAIL_VA (flag moved to word 9)

local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 45000)
local POKES_RAW = probe.getenv("LEGAIA_POKES", "")

local ITEMS = { 0xB9, 0x12, 0x1A }

local pokes = {}
for pair in string.gmatch(POKES_RAW, "[^,%s]+") do
    local a, v, w = string.match(pair, "([^:]+):([^:]+):?(.*)")
    if a and v then
        pokes[#pokes + 1] = { addr = tonumber(a), val = tonumber(v), byte = (w == "b") }
    end
end

local CSV = probe.csv_open(probe.out_path("custom_items_e2e.csv"), "tick,mode,note")

local function reg(r, name)
    local ok, v = pcall(function() return tonumber(r.GPR.n[name]) % 0x100000000 end)
    if ok then return v end
    return 0
end

local next_item = 1
local last_mode = -1
local tick_now = 0
local installed = false
local bag_seeded = false
local walk_released = false

local function bctx()
    local c = probe.read_u32(BCTX_PTR) or 0
    if c > 0x80000000 and c < 0x80200000 then return c end
    return nil
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    on_arm = function(ctx)
        probe.arm_breakpoint(SEED_HOOK, "Exec", 4, "seed", function()
            if (probe.read_u16(GAME_MODE) or 0) ~= 0x15 then return end
            local r = PCSX.getRegisters()
            local actor = reg(r, "s3")
            if actor < 0x80000000 or actor > 0x80200000 then return end
            local c = bctx()
            local cur = c and (probe.read_u8(c + 0x13) or -1) or -1
            if cur >= 0 and cur < 3 and next_item <= #ITEMS then
                local it = ITEMS[next_item]
                next_item = next_item + 1
                probe.write_u8(actor + 0x1DE, 1)
                probe.write_u8(actor + 0x1DF, it)
                probe.write_u8(actor + 0x1DD, 0)
                CSV:row("%d,0,SEED slot=%d FORCED item=0x%02X hp=%d mp=%d",
                    tick_now, cur,
                    it,
                    probe.read_u16(actor + 0x14C) or -1,
                    probe.read_u16(actor + 0x150) or -1)
            end
        end)
        probe.arm_breakpoint(CONV1_VA, "Exec", 4, "conv1", function()
            CSV:row("%d,0,CONV1", tick_now)
        end)
        probe.arm_breakpoint(CONV2_VA, "Exec", 4, "conv2", function()
            CSV:row("%d,0,CONV2 flag=%d", tick_now, probe.read_u8(FLAG_VA) or -1)
        end)
        probe.arm_breakpoint(MPSKIP_VA, "Exec", 4, "mpskip", function()
            CSV:row("%d,0,MPSKIP flag=%d", tick_now, probe.read_u8(FLAG_VA) or -1)
        end)
        probe.arm_breakpoint(FURY_VA, "Exec", 4, "fury", function()
            CSV:row("%d,0,FURY ARM", tick_now)
        end)
        probe.arm_breakpoint(ELIXIR_ARM, "Exec", 4, "elixir", function()
            CSV:row("%d,0,ELIXIR ARM", tick_now)
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        tick_now = elapsed
        local mode = probe.read_u16(GAME_MODE) or -1
        if not installed and elapsed >= 90 then
            for _, p in ipairs(pokes) do
                if p.byte then
                    probe.write_u8(p.addr, p.val)
                else
                    probe.write_u32(p.addr, p.val)
                end
            end
            installed = true
            CSV:row("%d,0x%X,pokes installed (%d)", elapsed, mode, #pokes)
        end
        if mode ~= last_mode then
            CSV:row("%d,0x%X,mode-change", elapsed, mode)
            last_mode = mode
        end
        if mode == 0x15 and not bag_seeded then
            -- Append the three items to the first empty bag slots.
            local wrote = 0
            for slot = 0, 0x1FE do
                if wrote >= #ITEMS then break end
                local a = BAG + slot * 2
                if (probe.read_u8(a) or 0) == 0 then
                    wrote = wrote + 1
                    probe.write_u8(a, ITEMS[wrote])
                    probe.write_u8(a + 1, 3)
                end
            end
            bag_seeded = true
            CSV:row("%d,0x%X,bag seeded (%d items x3)", elapsed, mode, #ITEMS)
        end
        if mode == 0x15 and not walk_released then
            pad.release(pad.BTN.UP); pad.release(pad.BTN.DOWN)
            pad.release(pad.BTN.LEFT); pad.release(pad.BTN.RIGHT)
            walk_released = true
        end
        if mode ~= 0x15 then
            walk_released = false
            local leg = math.floor(elapsed / 90) % 4
            pad.release(pad.BTN.UP); pad.release(pad.BTN.DOWN)
            pad.release(pad.BTN.LEFT); pad.release(pad.BTN.RIGHT)
            if leg == 0 then
                pad.force(pad.BTN.UP)
            elseif leg == 1 then
                pad.force(pad.BTN.RIGHT)
            elseif leg == 2 then
                pad.force(pad.BTN.DOWN)
            else
                pad.force(pad.BTN.LEFT)
            end
            return
        end
        -- UP-then-Cross drives the command menu so party actions commit.
        local phase = elapsed % 40
        if phase == 0 then
            pad.force(pad.BTN.UP)
        elseif phase == 4 then
            pad.release(pad.BTN.UP)
        elseif phase == 8 then
            pad.force(pad.BTN.CROSS)
        elseif phase == 14 then
            pad.release(pad.BTN.CROSS)
        end
        if elapsed % 60 == 0 then
            local cells = {}
            for slot = 0, 5 do
                local a = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
                if a > 0x80000000 and a < 0x80200000 then
                    cells[#cells + 1] = string.format(
                        "s%d q=%X tgt=%X act=%02X hp=%d mp=%d f=%d",
                        slot,
                        probe.read_u8(a + 0x1DE) or 0,
                        probe.read_u8(a + 0x1DD) or 0,
                        probe.read_u8(a + 0x1DF) or 0,
                        probe.read_u16(a + 0x14C) or 0,
                        probe.read_u16(a + 0x150) or 0,
                        probe.read_u8(a + 0x1F9) or 0)
                end
            end
            local c = bctx()
            local st = c and (probe.read_u8(c + 7) or -1) or -1
            local cur = c and (probe.read_u8(c + 0x13) or -1) or -1
            CSV:row("%d,0x%X,st=0x%02X cur=%d actors %s", elapsed, mode, st,
                cur, table.concat(cells, " | "))
        end
    end,
    on_done = function()
        CSV:row("%d,0x%X,done forced=%d/%d", tick_now, last_mode,
            next_item - 1, #ITEMS)
        CSV:close()
    end,
})
