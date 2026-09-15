-- autorun_w3a_cast_oracle.lua
--
-- Retail oracle for one slot-B cast-band body: a per-VSync timeline of the
-- module phase machine plus every damage-wrapper call and return, so the
-- ported tick bodies (legaia_engine_vm::cast_seru_ticks_a / _b /
-- cast_arm_ticks / cast_module_ticks) can be checked against retail frame by
-- frame. The phase dwell counts in timeline.csv ARE the per-arm frame gating
-- every static read had to disclose as capture-only.
--
-- Why an injection and not a menu drive: the *_summon_mid_cast corpus is
-- mednafen-only (no PCSX-Redux backup exists for any of those labels), and a
-- mid-cast state cannot show the arms before it anyway. So this probe starts
-- from a PRE-cast battle state (a command menu / Begin prompt), taps CROSS
-- until the acting seat's queued category byte +0x1DE lands, and rewrites the
-- queued action into the cast under test:
--
--     actor[+0x1DE] = 2 (Magic), actor[+0x1DF] = LEGAIA_SPELL,
--     actor[+0x1DD] = LEGAIA_TARGET_SEAT
--
-- Retail then pages the module in through the loader-B tracker 0x8007BC4C
-- (extraction = tracker + 895) and runs its own tick, so everything logged
-- after that is retail behaviour on retail bytes.
--
-- Outputs (probe.out_path, i.e. --out-dir):
--   timeline.csv   one row per VSync from cast-band entry: module phase,
--                  ctx scratch bytes and the actor-mirror fields the ports
--                  carry, for the caster / victim / summon seats plus every
--                  seat's HP.
--   wrappers.csv   one row per FUN_801DD0AC / 4B0 / 6B4 entry (a0..a3, ra)
--                  and one per matching epilogue (v0 = net damage).
--   records.csv    the four party records at inject time: the 32 learned
--                  action ids (+0x705 off 0x80084140) and the 32 parallel
--                  magic levels (+0x729), so the caster's magic level for
--                  this cast is derivable, plus 0x8007BD10[seat].
--
-- Env:
--   LEGAIA_SPELL         action id to force (default 0x83 Vera)
--   LEGAIA_TARGET_SEAT   actor-table seat to target (default 3)
--   LEGAIA_CASTER_SEAT   party seat whose queued action is rewritten (0)
--   LEGAIA_INJECT_AT     inject on this capture vsync instead of waiting for
--                        the category byte (0 = wait, the default)
--   LEGAIA_PRESS_UNTIL   keep tapping CROSS until this vsync (default 300)
--   LEGAIA_TAIL          vsyncs to keep logging after the done band (default 60)
--   LEGAIA_LABEL         free-text label written into manifest.txt
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local function env_addr(name)
    -- Address env vars are read with `tonumber`, which accepts the `0x` form
    -- directly - pass them as hex. A hand-converted decimal is how the first
    -- revision of this probe armed a breakpoint at 0x801C10D8 while believing
    -- it had armed 0x801F69D8, and a wrong-but-readable address answers 0
    -- rather than failing.
    local v = tonumber(os.getenv(name) or "")
    if v == nil then return 0 end
    return v
end

local SSTATE_PATH  = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES       = probe.getenv_num("LEGAIA_FRAMES", 1800)
local SPELL        = probe.getenv_num("LEGAIA_SPELL", 0x83)
local TARGET_SEAT  = probe.getenv_num("LEGAIA_TARGET_SEAT", 3)
local CASTER_SEAT  = probe.getenv_num("LEGAIA_CASTER_SEAT", 0)
local INJECT_AT    = probe.getenv_num("LEGAIA_INJECT_AT", 0)
local INJECT_STATE = probe.getenv_num("LEGAIA_INJECT_STATE", 0x0A)
local MP_TOPUP     = probe.getenv_num("LEGAIA_MP_TOPUP", 999)
-- The module's own tick entry, so the phase dwell can be counted in TICKS
-- (the unit the per-arm countdowns are in) instead of in VSyncs: the battle
-- SM does not advance once per VSync, so a VSync dwell is host timing.
local TICK_VA      = env_addr("LEGAIA_TICK_VA")
-- Optional module-resident countdown word to sample at each tick entry
-- (cast_arm_ticks.rs discloses one per capture-class body).
local CD_VA        = env_addr("LEGAIA_CD_VA")
local CD_VA2       = env_addr("LEGAIA_CD_VA2")
-- PROT 0898 trampoline for the spell under test (`0x801CF4EC` arm `i` is a
-- 16-byte stub at `0x801F1F3C + i*16`); 0 = derive from LEGAIA_SPELL.
local TRAMP_VA     = env_addr("LEGAIA_TRAMP_VA")
-- Record / actor seeding, applied at inject time so a body whose arithmetic
-- reads the caster's magic level or needs a victim that survives its own
-- multi-hit chain is measurable. Each is a legitimately-shaped value a real
-- playthrough would carry; retail code reads them unchanged.
local LEARN_LEVEL  = probe.getenv_num("LEGAIA_LEARN_LEVEL", 0)
local LEARN_SLOT   = probe.getenv_num("LEGAIA_LEARN_SLOT", 1)
local TARGET_HP    = probe.getenv_num("LEGAIA_TARGET_HP", 0)
local TARGET_MAXHP = probe.getenv_num("LEGAIA_TARGET_MAXHP", 0)
local CASTER_HP    = probe.getenv_num("LEGAIA_CASTER_HP", 0)
local PRESS_UNTIL  = probe.getenv_num("LEGAIA_PRESS_UNTIL", 300)
local TAIL         = probe.getenv_num("LEGAIA_TAIL", 60)
local LABEL        = probe.getenv("LEGAIA_LABEL", "w3a")

local ACTOR_TABLE = 0x801C9370
local CTX_PTR     = 0x8007BD24
local SLOTB       = 0x801F69D8
local LOADER_ID   = 0x8007BC4C
local SEAT_CHAR   = 0x8007BD10       -- per-seat 1-based party character id
local REC_BLOCK   = 0x80084140       -- record n = REC_BLOCK + n*0x414
local REC_STRIDE  = 0x414
local REC_LEARNED = 0x705            -- 32 learned action ids
local REC_LEVEL   = 0x729            -- 32 parallel magic levels
local CAST_TICK_TABLE = 0x801CF4EC   -- 32-slot cast-tick arm table (PROT 0898)
local MON_RECS    = 0x801C9348       -- per-monster-seat record pointer table
local SUM_REC     = 0x801C9358       -- the summon creature's record pointer

-- The three damage wrappers and their `jr ra` epilogues (v0 = net damage).
local WRAPPERS = {
    { name = "DD0AC", entry = 0x801DD0AC, exit = 0x801DD4A8 },
    { name = "DD4B0", entry = 0x801DD4B0, exit = 0x801DD6AC },
    { name = "DD6B4", entry = 0x801DD6B4, exit = 0x801DD85C },
}

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end

local function ctxp()
    local c = u32(CTX_PTR)
    if c < 0x80000000 or c >= 0x80200000 then return nil end
    return c
end
local function actor(slot)
    local p = u32(ACTOR_TABLE + slot * 4)
    if p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end
local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function regs()
    local r = PCSX.getRegisters()
    return (r.GPR and r.GPR.n) or {}
end

local timeline, wrappers, records, ticks
local injected, cast_seen, done_seen = false, false, false
local inject_vsync, cast_vsync, done_vsync = -1, -1, -1
local wrapper_n = 0
local tick_n = 0
local last_st7 = -1
local pending = {}       -- wrapper name -> the call row awaiting its epilogue

local function dump_records()
    if records == nil then return end
    for slot = 0, 3 do
        local base = REC_BLOCK + slot * REC_STRIDE
        local ids, lvls = {}, {}
        for i = 0, 31 do
            ids[#ids + 1]   = string.format("%02X", u8(base + REC_LEARNED + i))
            lvls[#lvls + 1] = string.format("%02X", u8(base + REC_LEVEL + i))
        end
        records:row("%d,%s,%s", slot, table.concat(ids, " "), table.concat(lvls, " "))
    end
    local seats = {}
    for seat = 0, 7 do seats[#seats + 1] = tostring(u8(SEAT_CHAR + seat)) end
    records:row("seatchar,%s,", table.concat(seats, " "))
    -- The raw monster records the Seru side-effect stager compares against
    -- (+0x0E AGL, +0x10 MP, +0x12 ATK, +0x14 UDF, +0x18 INT, +0x1A SPD,
    -- +0x1D element, +0x20 the flag three summon ticks read as a resist
    -- proxy), plus the summon creature's own record element.
    for seat = 3, 7 do
        local p = u32(MON_RECS + (seat - 3) * 4)
        if p >= 0x80000000 and p < 0x80200000 then
            records:row("monrec%d,agl=%d mp=%d atk=%d udf=%d int=%d spd=%d elem=%d f20=%d,",
                seat, u16(p + 0x0E), u16(p + 0x10), u16(p + 0x12), u16(p + 0x14),
                u16(p + 0x18), u16(p + 0x1A), u8(p + 0x1D), u8(p + 0x20))
        end
    end
    local sp = u32(SUM_REC)
    if sp >= 0x80000000 and sp < 0x80200000 then
        records:row("sumrec,elem=%d f20=%d atk=%d,", u8(sp + 0x1D), u8(sp + 0x20), u16(sp + 0x12))
    end
    -- The 32-slot cast-tick arm table and, for each arm, the `jal` target its
    -- 16-byte PROT 0898 trampoline jumps to - i.e. the module tick entry,
    -- read out of live RAM instead of out of a dump's printed VAs.
    for i = 0, 31 do
        local arm = u32(CAST_TICK_TABLE + i * 4)
        if arm >= 0x80000000 and arm < 0x80200000 then
            local target = 0
            for w = 0, 3 do
                local insn = u32(arm + w * 4)
                if insn >= 0x0C000000 and insn < 0x10000000 then
                    target = 0x80000000 + ((insn % 0x4000000) * 4)
                    break
                end
            end
            records:row("castarm%02d,stub=0x%08X jal=0x%08X,", i, arm, target)
        end
    end
end

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format("== w3a cast oracle == label=%s spell=0x%02X target=%d caster=%d",
            LABEL, SPELL, TARGET_SEAT, CASTER_SEAT))
        probe.env.write_manifest("autorun_w3a_cast_oracle.lua", {
            label = LABEL, sstate = SSTATE_PATH, spell = string.format("0x%02X", SPELL),
            target_seat = TARGET_SEAT, caster_seat = CASTER_SEAT, frames = FRAMES,
        })
        timeline = probe.csv_open(probe.out_path("timeline.csv"),
            "vsync,st7,phase,c278,c27a,c0d,c13,c1a,party,mons,c6d8,c6da,loader,slotb0," ..
            "cs_hp,cs_mp,cs_d10,cs_16e,cs_1da,cs_1dc,cs_1dd,cs_1de,cs_1df,cs_21d,cs_atk,cs_atkb,cs_def,cs_udf,cs_int," ..
            "vi_hp,vi_maxhp,vi_d10,vi_16e,vi_1da,vi_1dc,vi_21c,vi_21d,vi_atk,vi_atkb,vi_def,vi_udf,vi_int,vi_170," ..
            "su_1da,su_1dc,su_21c,su_21d,su_0c,hp0,hp1,hp2,hp3,hp4,hp5,hp6,hp7")
        wrappers = probe.csv_open(probe.out_path("wrappers.csv"),
            "vsync,kind,which,a0,a1,a2,a3,ra,v0,phase")
        records = probe.csv_open(probe.out_path("records.csv"), "slot,learned_ids,levels")
        ticks = probe.csv_open(probe.out_path("ticks.csv"),
            "tick,vsync,which,phase,c278,c6d8,w1,w2,ra")
        -- Five candidate tick entries cover the whole player-Seru set (the
        -- `0x801CF4EC` arm is the module's load base for most images but not
        -- all), plus whatever LEGAIA_TICK_VA names and the PROT 0898
        -- trampoline for the spell under test.
        local tick_sites = { 0x801F69D8, 0x801F69E8, 0x801F69EC, 0x801F69F0, 0x801F69F4 }
        if TICK_VA ~= 0 then tick_sites[#tick_sites + 1] = TICK_VA end
        if TRAMP_VA ~= 0 then tick_sites[#tick_sites + 1] = TRAMP_VA end
        local armed = {}
        for _, va in ipairs(tick_sites) do
            if not armed[va] then
                armed[va] = true
                probe.arm_breakpoint(va, "Exec", 4, string.format("tick%08X", va), function()
                    local cx = ctxp()
                    if cx == nil then return end
                    tick_n = tick_n + 1
                    local n = regs()
                    ticks:row("%d,%d,0x%08X,%d,%d,%d,%d,%d,0x%08X", tick_n, cast_vsync, va,
                        u8(cx + 0x279), u8(cx + 0x278), u8(cx + 0x6D8),
                        CD_VA ~= 0 and u32(CD_VA) or -1,
                        CD_VA2 ~= 0 and u32(CD_VA2) or -1, tou32(n.ra))
                end)
            end
        end

        for _, w in ipairs(WRAPPERS) do
            local nm = w.name
            probe.arm_breakpoint(w.entry, "Exec", 4, "w_" .. nm, function()
                local n = regs()
                wrapper_n = wrapper_n + 1
                local cx = ctxp()
                pending[nm] = {
                    a0 = tou32(n.a0), a1 = tou32(n.a1), a2 = tou32(n.a2),
                    a3 = tou32(n.a3), ra = tou32(n.ra),
                }
                wrappers:row("%d,call,%s,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,,%d",
                    cast_vsync, nm, pending[nm].a0, pending[nm].a1, pending[nm].a2,
                    pending[nm].a3, pending[nm].ra, cx and u8(cx + 0x279) or -1)
            end)
            probe.arm_breakpoint(w.exit, "Exec", 4, "x_" .. nm, function()
                local n = regs()
                local p = pending[nm]
                local cx = ctxp()
                wrappers:row("%d,ret,%s,%s,%s,%s,%s,%s,0x%08X,%d",
                    cast_vsync, nm,
                    p and string.format("0x%08X", p.a0) or "",
                    p and string.format("0x%08X", p.a1) or "",
                    p and string.format("0x%08X", p.a2) or "",
                    p and string.format("0x%08X", p.a3) or "",
                    p and string.format("0x%08X", p.ra) or "",
                    tou32(n.v0), cx and u8(cx + 0x279) or -1)
                pending[nm] = nil
            end)
        end
        return {}
    end,

    on_capture = function(c, elapsed)
        local cx = ctxp()
        local cs = actor(CASTER_SEAT)
        if cx == nil or cs == nil then return end

        probe.pad_release(probe.BTN.CROSS)
        if not injected and elapsed < PRESS_UNTIL then
            local sub = elapsed % 60
            if sub >= 30 and sub < 34 then probe.pad_force(probe.BTN.CROSS) end
        end

        local cat = u8(cs + 0x1DE)
        local st7 = u8(cx + 7)
        -- Inject at the action-seed gate, NOT at the Begin/Reselect confirm.
        -- The command-flow SM (ctx+6) validates the queued actions at its
        -- confirm arm 0x6E, and a Magic action the caster has not learned
        -- parks it there forever; ctx[7] == INJECT_STATE is past that gate and
        -- before state 0x0C reads actor[+0x1DE] (battle-action.md).
        -- ctx[+0x13] is the ACTING seat; a monster often acts first, and an
        -- injection into a seat that is not acting is simply ignored.
        local acting = u8(cx + 0x13)
        local want = (INJECT_AT > 0 and elapsed >= INJECT_AT)
            or (INJECT_AT == 0 and st7 == INJECT_STATE and acting == CASTER_SEAT)
        if want and not injected then
            injected = true
            inject_vsync = elapsed
            if LEARN_LEVEL > 0 then
                local char_idx = u8(SEAT_CHAR + CASTER_SEAT)
                if char_idx > 0 then
                    local base = REC_BLOCK + (char_idx - 1) * REC_STRIDE
                    probe.write_u8(base + REC_LEARNED + LEARN_SLOT, SPELL)
                    probe.write_u8(base + REC_LEVEL + LEARN_SLOT, LEARN_LEVEL)
                end
            end
            dump_records()
            if MP_TOPUP > 0 then probe.write_u16(cs + 0x150, MP_TOPUP) end
            -- +0x172 is the DISPLAYED HP the party HUD ramps towards, and the
            -- 0x51 exit gate FUN_801E7250 holds the action band while a PARTY
            -- target's +0x14C differs from it (battle-action.md). Poking live
            -- HP without the display copy parks the battle at 0x51 forever.
            if CASTER_HP > 0 then
                probe.write_u16(cs + 0x14C, CASTER_HP)
                probe.write_u16(cs + 0x172, CASTER_HP)
            end
            local v = actor(TARGET_SEAT)
            if v ~= nil then
                if TARGET_MAXHP > 0 then probe.write_u16(v + 0x14E, TARGET_MAXHP) end
                if TARGET_HP > 0 then
                    probe.write_u16(v + 0x14C, TARGET_HP)
                    probe.write_u16(v + 0x172, TARGET_HP)
                end
            end
            probe.write_u8(cs + 0x1DE, 2)
            probe.write_u8(cs + 0x1DF, SPELL)
            probe.write_u8(cs + 0x1DD, TARGET_SEAT)
            PCSX.log(string.format("[inject t%d] +0x1DE->2 +0x1DF=0x%02X +0x1DD=%d ctx7=0x%02X ctx6=0x%02X",
                elapsed, SPELL, TARGET_SEAT, st7, u8(cx + 6)))
        end
        if injected and not cast_seen and (st7 == 0x28 or (st7 >= 0x6E and st7 <= 0x71)) then
            cast_seen = true
            PCSX.log(string.format("[cast t%d] band entered st7=0x%02X loader=%d",
                elapsed, st7, u8(LOADER_ID)))
        end
        local flow = u8(cx + 6)
        local key = flow * 256 + st7
        if key ~= last_st7 then
            PCSX.log(string.format("[flow t%d] ctx6=0x%02X ctx7=0x%02X acting=%d cat=%d id=0x%02X tgt=%d ph=%d loader=%d",
                elapsed, flow, st7, u8(cx + 0x13), cat, u8(cs + 0x1DF), u8(cs + 0x1DD),
                u8(cx + 0x279), u8(LOADER_ID)))
            last_st7 = key
        end
        if not injected then return end
        cast_vsync = elapsed

        local vi = actor(TARGET_SEAT)
        local su = actor(7)
        local function A(p, off, w)
            if p == nil then return -1 end
            if w == 1 then return u8(p + off) end
            if w == 4 then return u32(p + off) end
            return u16(p + off)
        end
        local hp = {}
        for s = 0, 7 do
            local a = actor(s)
            hp[#hp + 1] = a and u16(a + 0x14C) or -1
        end

        timeline:row(
            "%d,0x%02X,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,0x%08X," ..
            "%d,%d,%d,%d,%d,%d,%d,%d,0x%02X,%d,%d,%d,%d,%d,%d," ..
            "%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d," ..
            "%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
            elapsed, st7, u8(cx + 0x279), u8(cx + 0x278), u8(cx + 0x27A),
            u8(cx + 0x0D), u8(cx + 0x13), u8(cx + 0x1A), u8(cx + 0), u8(cx + 1),
            u8(cx + 0x6D8), u8(cx + 0x6DA), u8(LOADER_ID), u32(SLOTB),
            A(cs, 0x14C), A(cs, 0x150), A(cs, 0x10), A(cs, 0x16E), A(cs, 0x1DA, 1),
            A(cs, 0x1DC, 1), A(cs, 0x1DD, 1), A(cs, 0x1DE, 1), A(cs, 0x1DF, 1),
            A(cs, 0x21D, 1), A(cs, 0x158), A(cs, 0x15A), A(cs, 0x15C), A(cs, 0x160), A(cs, 0x168),
            A(vi, 0x14C), A(vi, 0x14E), A(vi, 0x10), A(vi, 0x16E), A(vi, 0x1DA, 1),
            A(vi, 0x1DC, 1), A(vi, 0x21C, 1), A(vi, 0x21D, 1), A(vi, 0x158), A(vi, 0x15A),
            A(vi, 0x15C), A(vi, 0x160), A(vi, 0x168), A(vi, 0x170),
            A(su, 0x1DA, 1), A(su, 0x1DC, 1), A(su, 0x21C, 1), A(su, 0x21D, 1), A(su, 0x0C, 4),
            hp[1], hp[2], hp[3], hp[4], hp[5], hp[6], hp[7], hp[8])

        if cast_seen and not done_seen and (st7 == 0x50 or st7 == 0x51 or st7 == 0x5A) then
            done_seen = true
            done_vsync = elapsed
            -- Second pass: the summon record pointer is only populated once
            -- the creature is staged, so it reads as garbage at inject time.
            records:row("--- at done ---,,")
            dump_records()
            PCSX.log(string.format("[done t%d] st7=0x%02X wrappers=%d", elapsed, st7, wrapper_n))
        end
        if done_seen and elapsed > done_vsync + TAIL then c.request_quit = true end
    end,

    on_done = function()
        PCSX.log(string.format("[w3a] inject=%d cast=%s done=%s wrapper_calls=%d ticks=%d",
            inject_vsync, tostring(cast_seen), tostring(done_seen), wrapper_n, tick_n))
        if timeline then timeline:close() end
        if wrappers then wrappers:close() end
        if records then records:close() end
        if ticks then ticks:close() end
    end,
})
