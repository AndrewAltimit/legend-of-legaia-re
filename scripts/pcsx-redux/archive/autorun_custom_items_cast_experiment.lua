-- autorun_custom_items_cast_experiment.lua
--
-- MECHANISM EXPERIMENT for the custom-items Tears (no disc patch involved):
-- can a PARTY actor execute (a) a capture-class Delilas signature cast
-- (0x79/0x7A/0x7B, modules PROT 958/959/960 - authored for a monster
-- caster) and (b) a Ra-Seru summon it hasn't learned (0x9E/0x9F/0xA0)?
--
-- Method: exec BP on the action-seed category dispatch at 0x801E2D60
-- (FUN_801E295C state 0xC, `lbu v1,0x1de(s3)`). When the acting actor is a
-- party slot, hijack the committed action exactly the way the planned
-- item->cast conversion hook would: +0x1DE=2 (magic), +0x1DF=<next test
-- spell>, +0x1DD=3 (first enemy slot). One spell per party action-seed,
-- from LEGAIA_CAST_LIST. Receipts: the module pager BP (FUN_8003EC70), MP
-- before/after (the unconditional state-0x3C deduct - underflow check),
-- per-tick actor anim/phase rows (wedge shape), HP deltas (damage landed).
--
-- Launch (retail disc, mid-battle state):
--   LEGAIA_CAST_LIST="0x79,0x9e,0x7a,0x9f,0x7b,0xa0" \
--   bash scripts/pcsx-redux/run_probe.sh --iso "$LEGAIA_DISC_BIN" \
--     --scenario party_basic_attack_vs_gobu_gobu \
--     --lua scripts/pcsx-redux/autorun_custom_items_cast_experiment.lua
--
-- Output: custom_items_cast.csv  tick,mode,note

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE   = 0x8007B83C
local ACTOR_TABLE = 0x801C9370
local BCTX_PTR    = 0x8007BD24
local SEED_DISPATCH = 0x801E2D60 -- lbu v1,0x1de(s3) before the category jr
local MODULE_PAGER  = 0x8003EC70 -- slot-B overlay pager (capture modules + summons)
local MALLOC_FAIL   = 0x800178B8

local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 24000)
local LIST_RAW = probe.getenv("LEGAIA_CAST_LIST", "0x79,0x9e,0x7a,0x9f,0x7b,0xa0")
-- Diagnostic: top the caster's current MP to this value right before the
-- forced cast (0 = leave alone). Discriminates "the state-0x3C unconditional
-- MP deduct underflowed and poisoned the summon-effect start" from a
-- structural park. Probe-only; the real conversion hook skips the deduct.
local MP_TOP = probe.getenv_num("LEGAIA_MP_TOP", 0)
-- LEGAIA_WALK=1: start from a field/world-map state and walk (alternating
-- held directions) until a random encounter flips mode to 0x15; the seed BP
-- only hijacks in battle mode (outside battle the 0x801E2D60 VA hosts some
-- other overlay's code, so the callback must self-gate).
local WALK = probe.getenv_num("LEGAIA_WALK", 0)
-- LEGAIA_NO_MASH=1: don't drive the battle menus at all - a human at the
-- controller commits actions; the seed BP still hijacks the first party
-- commits into the forced casts.
local NO_MASH = probe.getenv_num("LEGAIA_NO_MASH", 0)
-- LEGAIA_CAST_BY_SLOT=1: instead of consuming LEGAIA_CAST_LIST in order,
-- force spell = 0x9E + acting party slot (Vahn->Meta, Noa->Terra,
-- Gala->Ozma) - the caster-matched Ra-Seru summon hypothesis.
local BY_SLOT = probe.getenv_num("LEGAIA_CAST_BY_SLOT", 0)

local spells = {}
for tok in string.gmatch(LIST_RAW, "[^,%s]+") do
    spells[#spells + 1] = tonumber(tok)
end

local CSV = probe.csv_open(probe.out_path("custom_items_cast.csv"),
    "tick,mode,note")

local function reg(r, name)
    local ok, v = pcall(function() return tonumber(r.GPR.n[name]) % 0x100000000 end)
    if ok then return v end
    return 0
end

local next_spell = 1
local last_mode = -1
local malloc_fails = 0
local tick_now = 0
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
        probe.arm_breakpoint(SEED_DISPATCH, "Exec", 4, "seed", function()
            if (probe.read_u16(GAME_MODE) or 0) ~= 0x15 then return end
            local r = PCSX.getRegisters()
            local actor = reg(r, "s3")
            if actor < 0x80000000 or actor > 0x80200000 then return end
            local c = bctx()
            local cur = c and (probe.read_u8(c + 0x13) or -1) or -1
            local cat = probe.read_u8(actor + 0x1DE) or -1
            if cur >= 0 and cur < 3 and next_spell <= #spells then
                local sp = spells[next_spell]
                if BY_SLOT ~= 0 then
                    sp = 0x9E + cur
                end
                next_spell = next_spell + 1
                if MP_TOP > 0 then
                    probe.write_u16(actor + 0x150, MP_TOP)
                end
                probe.write_u8(actor + 0x1DE, 2)
                probe.write_u8(actor + 0x1DF, sp)
                probe.write_u8(actor + 0x1DD, 3)
                CSV:row("%d,0,SEED slot=%d cat_was=%d FORCED spell=0x%02X mp=%d",
                    tick_now, cur, cat, sp, probe.read_u16(actor + 0x150) or -1)
            else
                CSV:row("%d,0,SEED slot=%d cat=%d act=0x%02X (untouched)",
                    tick_now, cur, cat, probe.read_u8(actor + 0x1DF) or -1)
            end
        end)
        probe.arm_breakpoint(MODULE_PAGER, "Exec", 4, "pager", function()
            local r = PCSX.getRegisters()
            CSV:row("%d,0,MODULE PAGE a0=%d ra=0x%08X", tick_now,
                reg(r, "a0"), reg(r, "ra"))
        end)
        probe.arm_breakpoint(MALLOC_FAIL, "Exec", 4, "malloc_fail", function()
            malloc_fails = malloc_fails + 1
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        tick_now = elapsed
        local mode = probe.read_u16(GAME_MODE) or -1
        if mode ~= last_mode then
            CSV:row("%d,0x%X,mode-change", elapsed, mode)
            last_mode = mode
        end
        if WALK ~= 0 and mode == 0x15 and not walk_released then
            -- Entering battle: release whatever direction the walk held, or
            -- it keeps cycling the target cursor forever.
            pad.release(pad.BTN.UP); pad.release(pad.BTN.DOWN)
            pad.release(pad.BTN.LEFT); pad.release(pad.BTN.RIGHT)
            walk_released = true
        end
        if WALK ~= 0 and mode ~= 0x15 then
            walk_released = false
            -- Walk in a small loop until an encounter rolls.
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
        if NO_MASH == 0 then
            -- UP-then-Cross (the dome probes' proven pattern) drives the
            -- command menu so party actions keep committing.
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
        end
        if elapsed % 60 == 0 and mode == 0x15 then
            local cells = {}
            for slot = 0, 5 do
                local a = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
                if a > 0x80000000 and a < 0x80200000 then
                    cells[#cells + 1] = string.format(
                        "s%d anim=%02X/%02X q=%X tgt=%X act=%02X hp=%d mp=%d",
                        slot,
                        probe.read_u8(a + 0x1D9) or 0,
                        probe.read_u8(a + 0x1DA) or 0,
                        probe.read_u8(a + 0x1DE) or 0,
                        probe.read_u8(a + 0x1DD) or 0,
                        probe.read_u8(a + 0x1DF) or 0,
                        probe.read_u16(a + 0x14C) or 0,
                        probe.read_u16(a + 0x150) or 0)
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
        CSV:row("%d,0x%X,done malloc_fails=%d forced=%d/%d", tick_now,
            last_mode, malloc_fails, next_spell - 1, #spells)
        CSV:close()
    end,
})
