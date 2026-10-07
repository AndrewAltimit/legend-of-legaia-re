-- autorun_shiny_fade_trace.lua
--
-- Pin which actor the SUMMON-CREATURE mesh is drawn as, for the shiny-Seru
-- transparency tell (`legaia_patcher::shiny_seru`). The shiny routine floors the
-- summon actor's fade byte (`+0x226 = 1`), which the game's per-primitive fade
-- modulator `FUN_8004A908` turns into a semi-transparent draw. In-vivo the byte
-- IS set (confirmed via mednafen RAM: caster `+0x1dd` = summon slot 3, slot-3
-- `+0x226 = 1`) but the creature still renders opaque - so the visible mesh is
-- emitted reading a DIFFERENT actor's `+0x226` than the one we floor.
--
-- This probe settles it. It breakpoints the single draw-time `+0x226` reader
-- (`lbu v0,0x226(s1)` at 0x8004AD0C inside FUN_8004A908) and, for every hit
-- during a Seru cast, records the actor pointer `s1`, its fade byte, and which
-- battle-actor-table slot (if any) `s1` matches. Cross-referenced with the
-- caster's summon slot (`caster+0x1dd`), the output says exactly which actor the
-- creature mesh uses - the actor whose `+0x226` the transparency routine should
-- floor.
--
-- USAGE (the breakpoint fires whenever an actor is drawn with the fade path, so
-- aim the save state at a SHINY Seru cast in progress):
--   LEGAIA_SSTATE=~/Tools/pcsx-redux/SCUS94254.sstateN \
--     scripts/pcsx-redux/run_probe.sh autorun_shiny_fade_trace.lua
--
-- Output: scripts/pcsx-redux/out/shiny_fade_trace.csv  (one row per distinct
-- (actor ptr, slot, fade) seen, with a hit count), plus a PCSX.log summary.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 300)

local FADE_READER = 0x8004AD0C -- lbu v0,0x226(s1) inside FUN_8004A908
local ACTOR_TABLE = 0x801C9370 -- battle actor pointer table (16 slots)
local CTX_PTR     = 0x8007BD24 -- *CTX_PTR -> battle ctrl; +0x13 = caster slot
local FADE_OFF    = 0x226
local SUMMON_OFF  = 0x1DD       -- caster+0x1dd = summon's actor-table slot

local function u8(a) return probe.read_u8(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end

-- Resolve a pointer to its battle-actor-table slot (0..15), or -1.
local function slot_of(ptr)
    for i = 0, 15 do
        if u32(ACTOR_TABLE + i * 4) == ptr then return i end
    end
    return -1
end

local FADE_FUNC   = 0x8004A908   -- FUN_8004A908 entry (per-actor fade modulator)
local MANAGER_FN  = 0x801D71B8   -- FUN_801d71b8 entry (summon fade-state manager)
local HOOK_TAIL   = 0x801D7180   -- the jal site our routine detours (our hook point)

-- key "ptr|fade" -> { ptr, slot, fade, hits }
local seen = {}
local order = {}
local raw_reader_hits = 0
local raw_entry_hits = 0
local SUMMON_PTR = 0x800ED264 -- slot 3 in this capture (caster+0x1dd); the summon
local timeline = {}           -- ordered events for the summon actor (first ~50)
local seq = 0
local function ev(kind)
    seq = seq + 1
    if #timeline < 60 then timeline[#timeline + 1] = string.format("%03d %s", seq, kind) end
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        PCSX.log("== shiny fade trace: breakpoints on FUN_8004A908 entry + 0x226 read ==")
        -- Entry BP: count every call (diagnostic - is the fade path used at all?).
        probe.arm_breakpoint(FADE_FUNC, "Exec", 4, "fade_entry", function()
            raw_entry_hits = raw_entry_hits + 1
        end)
        -- Manager + our-hook markers, to see write-vs-read ordering for the summon.
        probe.arm_breakpoint(0x801D5854, "Exec", 4, "entry5854", function()
            local ctrl = u32(CTX_PTR)
            local ci = (ctrl >= 0x80000000) and u8(ctrl + 0x13) or -1
            local caster = (ci >= 0 and ci < 16) and u32(ACTOR_TABLE + ci * 4) or 0
            local act = (caster >= 0x80000000) and u8(caster + 0x1DF) or -1
            local sslot = (caster >= 0x80000000) and u8(caster + SUMMON_OFF) or -1
            local sptr = (sslot >= 0 and sslot < 16) and (u32(ACTOR_TABLE + sslot * 4) % 0x100000000) or 0
            local fade = (sptr >= 0x80000000) and u8(sptr + FADE_OFF) or -1
            ev(string.format("ENTRY action=0x%02X summon_slot=%d summon_fade=%d", act, sslot, fade))
        end)
        probe.arm_breakpoint(MANAGER_FN, "Exec", 4, "manager", function() ev("WRITE-mgr(801d71b8)") end)
        probe.arm_breakpoint(HOOK_TAIL, "Exec", 4, "hooktail", function() ev("OUR-HOOK(801d7180)") end)
        probe.arm_breakpoint(FADE_READER, "Exec", 4, "fade_read", function()
            raw_reader_hits = raw_reader_hits + 1
            local r = PCSX.getRegisters()
            local s1 = bit.band(tonumber(r.GPR.n.s1) or 0, 0xFFFFFFFF) % 0x100000000
            local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF) % 0x100000000
            local fade = (s1 >= 0x80000000 and s1 < 0x80200000) and u8(s1 + FADE_OFF) or -1
            local slot = (s1 >= 0x80000000 and s1 < 0x80200000) and slot_of(s1) or -1
            -- mark a couple distinctive summon-actor fields for identification
            local f06 = (s1 >= 0x80000000 and s1 < 0x80200000) and u8(s1 + 0x06) or -1
            local key = string.format("%08X", s1)
            local e = seen[key]
            if e then
                e.hits = e.hits + 1
            else
                e = { ptr = s1, slot = slot, fade = fade, hits = 1, ra = ra, f06 = f06 }
                seen[key] = e
                order[#order + 1] = key
            end
        end)
        return {}
    end,
    on_summary = function()
        -- Context: caster slot + its summon slot, so the reader rows can be read
        -- against "which actor we floor".
        local ctrl = u32(CTX_PTR)
        local ci = (ctrl ~= 0) and u8(ctrl + 0x13) or -1
        local caster = (ci >= 0) and u32(ACTOR_TABLE + ci * 4) or 0
        local summon_slot = (caster ~= 0) and u8(caster + SUMMON_OFF) or -1
        local summon_ptr = (summon_slot >= 0 and summon_slot < 16)
            and u32(ACTOR_TABLE + summon_slot * 4) or 0
        PCSX.log(string.format(
            "context: caster slot=%d (ptr %08X)  summon slot=%d (ptr %08X)  <- the actor the routine floors",
            ci, caster, summon_slot, summon_ptr))
        PCSX.log(string.format(
            "raw hits: FUN_8004A908 entry=%d, +0x226 read=%d", raw_entry_hits, raw_reader_hits))
        PCSX.log("== event timeline for the summon (write-mgr / our-hook / read-summon) ==")
        for _, line in ipairs(timeline) do PCSX.log("  " .. line) end

        local csv = probe.csv_open(
            probe.out_path("shiny_fade_trace.csv"),
            "actor_ptr,table_slot,fade,f06,hits,caller_ra")
        PCSX.log("  actor_ptr  slot  fade  +0x06  hits  caller(ra)")
        for _, key in ipairs(order) do
            local e = seen[key]
            PCSX.log(string.format("  %08X   %3d  %4d   0x%02X  %5d  %08X",
                e.ptr, e.slot, e.fade, e.f06, e.hits, e.ra))
            csv:row("0x%08X,%d,%d,0x%02X,%d,0x%08X", e.ptr, e.slot, e.fade, e.f06, e.hits, e.ra)
        end
        csv:close()
        PCSX.log("== wrote scripts/pcsx-redux/out/shiny_fade_trace.csv ==")
        PCSX.log("Read: the creature mesh = the row(s) with many hits + a real model;")
        PCSX.log("if its fade=0 while is_summon=YES has fade=1, we're flooring the wrong actor.")
    end,
})
