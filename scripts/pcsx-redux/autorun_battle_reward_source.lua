-- autorun_battle_reward_source.lua
--
-- Pins the battle-victory REWARD path (EXP / gold / drop) and, via the
-- writing instruction's registers, the monster-record offsets the spoils
-- code reads.
--
-- Background (docs/subsystems/battle-formulas.md):
--   The reward COMMIT is FUN_80026018: party XP bank 0x800845A4 +=
--   accumulator 0x80084440 (clamp 9,999,999); party gold 0x8008459C
--   commits the same way. But the per-monster READ that fills the
--   accumulator is not statically locatable (the commit is shared by the
--   minigames, the battle FSM never references the accumulator, and
--   neither monster-init nor the loader reads the reward fields). This
--   probe catches the WRITES at runtime during a battle win.
--
-- Method:
--   Write BPs on the XP accumulator, gold, and party-XP bank. Each hit
--   logs the writing PC + every GPR + the new value. The PC localizes the
--   summation function; the GPRs expose the source pointer (monster record
--   base + reward offset). Because Gimard (id 10) is a LONE enemy in this
--   fight, the staged accumulator == Gimard's EXP and the gold delta ==
--   Gimard's gold, so the values cross-reference directly against the
--   record candidate fields (+0x44/+0x46/+0x48).
--
-- Run (slot 9 = Gimard at death / end of fight):
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate9 \
--   LEGAIA_FRAMES=1200 \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_battle_reward_source.lua \
--       timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate9")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1200)
local OUT_PATH = probe.out_path("battle_reward_source.csv")

local XP_ACCUM  = 0x80084440 -- SC base 0x80084140 + 0x300 (XP accumulator)
local GOLD       = 0x8008459C -- party gold (24-bit, cap 9,999,999)
local PARTY_XP   = 0x800845A4 -- party XP bank (SC + 0x464)
local GOLD_ACCUM = 0x8008443C -- SC + 0x2FC (candidate gold accumulator, sib of XP)
local FUN_80026018 = 0x80026018 -- XP/gold commit
local FUN_80054CB0 = 0x80054CB0 -- monster init (record -> actor); record base = a0

-- GPR register names in PCSX.getRegisters().GPR.n.*
-- PCSX-Redux exposes the frame pointer as s8 (not fp); reading a missing
-- member throws, so dump defensively.
local GPR_NAMES = {
    "at", "v0", "v1", "a0", "a1", "a2", "a3",
    "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7",
    "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "t8", "t9", "k0", "k1", "gp", "sp", "s8", "ra",
}

local function gpr_dump(r)
    local parts = {}
    for _, nm in ipairs(GPR_NAMES) do
        local ok, v = pcall(function()
            return bit.band(tonumber(r.GPR.n[nm]) or 0, 0xFFFFFFFF)
        end)
        if ok then parts[#parts + 1] = string.format("%s=%08X", nm, v) end
    end
    return table.concat(parts, " ")
end

local csv = probe.csv_open(OUT_PATH, "tick,target,pc,value")

-- The record base for Gimard (a0 at monster-init), so we can re-read its
-- reward-candidate fields once we know the staged totals.
local gimard_rec = 0

local function log_record_candidates(rec)
    if rec == 0 or not probe.in_ram(rec, 0x60) then return end
    PCSX.log(string.format(
        "[rew] gimard rec=0x%08X candidates: +0x1C=%d +0x1E=%d +0x20(u32)=%d "
        .. "+0x44=%d +0x46=%d +0x48=%d +0x48(u32)=%d",
        rec,
        probe.read_u16(rec + 0x1C), probe.read_u16(rec + 0x1E),
        probe.read_u32(rec + 0x20),
        probe.read_u16(rec + 0x44), probe.read_u16(rec + 0x46),
        probe.read_u16(rec + 0x48), probe.read_u32(rec + 0x48)))
end

local hits = { XP_ACCUM = 0, GOLD = 0, PARTY_XP = 0, GOLD_ACCUM = 0, commit = 0 }
local HIT_CAP = 24

local function watch(addr, label)
    probe.arm_breakpoint(addr, "Write", 4, label, function()
        if hits[label] >= HIT_CAP then return end
        hits[label] = hits[label] + 1
        local r = PCSX.getRegisters()
        local pc = bit.band(tonumber(r.pc) or 0, 0xFFFFFFFF)
        local val = probe.read_u32(addr)
        csv:row("%d,%s,0x%08X,0x%08X", hits[label], label, pc, val)
        PCSX.log(string.format("[rew] WRITE %-10s #%d pc=0x%08X new=0x%08X (%d)",
            label, hits[label], pc, val, val))
        PCSX.log(string.format("[rew]   GPR %s", gpr_dump(r)))
    end)
end

local logged = { [0] = false }

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        local bps = {}
        watch(XP_ACCUM, "XP_ACCUM");   table.insert(bps, { addr = XP_ACCUM, name = "XP_ACCUM" })
        watch(GOLD, "GOLD");           table.insert(bps, { addr = GOLD, name = "GOLD" })
        watch(PARTY_XP, "PARTY_XP");   table.insert(bps, { addr = PARTY_XP, name = "PARTY_XP" })
        watch(GOLD_ACCUM, "GOLD_ACCUM"); table.insert(bps, { addr = GOLD_ACCUM, name = "GOLD_ACCUM" })

        -- Commit confirm + accumulator snapshot.
        table.insert(bps, { addr = FUN_80026018, name = "commit" })
        probe.arm_breakpoint(FUN_80026018, "Exec", 4, "commit", function()
            hits.commit = hits.commit + 1
            PCSX.log(string.format(
                "[rew] COMMIT #%d  XP_ACCUM=%d  PARTY_XP=%d  GOLD=%d  GOLD_ACCUM=%d",
                hits.commit, probe.read_u32(XP_ACCUM), probe.read_u32(PARTY_XP),
                probe.read_u32(GOLD), probe.read_u32(GOLD_ACCUM)))
        end)

        -- Catch Gimard's record base if the fight re-inits (id 10, a1=slot).
        table.insert(bps, { addr = FUN_80054CB0, name = "monster_init" })
        probe.arm_breakpoint(FUN_80054CB0, "Exec", 4, "monster_init", function()
            local r = PCSX.getRegisters()
            local rec = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
            if probe.in_ram(rec, 0x60) then
                gimard_rec = rec
                log_record_candidates(rec)
            end
        end)

        return bps
    end,

    on_capture = function(_ctx, elapsed)
        local sec = math.floor(elapsed)
        if not logged[sec] then
            logged[sec] = true
            PCSX.log(string.format(
                "[rew] t=%ds  XP_ACCUM=%d  PARTY_XP=%d  GOLD=%d  GOLD_ACCUM=%d",
                sec, probe.read_u32(XP_ACCUM), probe.read_u32(PARTY_XP),
                probe.read_u32(GOLD), probe.read_u32(GOLD_ACCUM)))
        end
    end,

    on_done = function()
        csv:close()
        if gimard_rec ~= 0 then log_record_candidates(gimard_rec) end
        PCSX.log(string.format(
            "=== reward probe: XP_ACCUM writes=%d GOLD writes=%d PARTY_XP writes=%d "
            .. "GOLD_ACCUM writes=%d commits=%d ===",
            hits.XP_ACCUM, hits.GOLD, hits.PARTY_XP, hits.GOLD_ACCUM, hits.commit))
        PCSX.log(string.format(
            "[rew] FINAL  XP_ACCUM=%d  PARTY_XP=%d  GOLD=%d  GOLD_ACCUM=%d",
            probe.read_u32(XP_ACCUM), probe.read_u32(PARTY_XP),
            probe.read_u32(GOLD), probe.read_u32(GOLD_ACCUM)))
    end,
})
