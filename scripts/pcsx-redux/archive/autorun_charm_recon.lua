-- autorun_charm_recon.lua
--
-- Recon for the enemy-ally "charm" playtest. Confirms, on a live battle state,
-- the exact fields the randomizer's enemy_ally injection reads/writes:
--   - ctx[0] = party count, ctx[1] = monster count  (_DAT_8007BD24)
--   - per battle-actor (table 0x801C9370): +0x14C liveness, +0x16E flags,
--     +0x1DD target slot, +0x1DE action category, +0x172 HP
--   - the overlay victory-check word at 0x801E6638 (expect andi v0,v0,0x4 =
--     0x30420004), proving the patch site is correct in live RAM.
-- No input, no poking; short window; quits.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")

local ACTOR_TABLE = 0x801C9370
local CTX_PTR     = 0x8007BD24
local GMODE       = 0x8007B83C
local VICTORY_VA  = 0x801E6638

local function s16(v) if v >= 0x8000 then return v - 0x10000 end return v end
local function u32(a) return probe.read_u32(a) or 0 end
local function ok_ptr(p) return p >= 0x80000000 and p < 0x80200000 end

local function dump(tag)
    local gm = probe.read_u8(GMODE) or 0
    local ctx = u32(CTX_PTR)
    local pc = ok_ptr(ctx) and (probe.read_u8(ctx + 0) or 0) or -1
    local mc = ok_ptr(ctx) and (probe.read_u8(ctx + 1) or 0) or -1
    local vword = probe.read_u32(VICTORY_VA) or 0
    PCSX.log(string.format(
        "== charm recon %s == gm=0x%02X ctx=0x%08X party_count=%d monster_count=%d victory_word=0x%08X (expect 0x30420004)",
        tag, gm, ctx, pc, mc, vword))
    for slot = 0, 7 do
        local p = u32(ACTOR_TABLE + slot * 4)
        if ok_ptr(p) then
            local live = probe.read_u16(p + 0x14C) or 0
            local flags = probe.read_u16(p + 0x16E) or 0
            local tgt = probe.read_u8(p + 0x1DD) or 0
            local cat = probe.read_u8(p + 0x1DE) or 0
            local hp = s16(probe.read_u16(p + 0x172) or 0)
            local band = (slot < pc) and "PARTY " or "MONSTER"
            PCSX.log(string.format(
                "  slot %d %s -> %08X live=%d flags=0x%04X(ai380=%s) target=%d cat=%d hp=%d",
                slot, band, p, live, flags,
                (flags % 0x400 >= 0x380) and "Y" or "n", tgt, cat, hp))
        end
    end
end

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = 200,
    on_arm = function() return {} end,
    on_capture = function(c, elapsed)
        if elapsed == 60 or elapsed == 130 then dump("t" .. elapsed) end
        if elapsed >= 150 then c.request_quit = true end
    end,
})
