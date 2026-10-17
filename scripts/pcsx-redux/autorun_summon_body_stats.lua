-- autorun_summon_body_stats.lua
--
-- Live check of WHICH record the summon body's stats come from.
--
-- The summon damage roll FUN_801DD0AC(_, 7, seat) reads the slot-7 actor's
-- HP (+0x14C) and INT (+0x168). FUN_801F19EC installs the streamed
-- summon.dat actor record at 0x801C9358 and calls FUN_80054CB0(record, 4),
-- the monster record -> actor copy, for seat 4 (= slot 7). This probe logs,
-- at every slot-7 roll, the actor's HP / INT / defence pair next to the
-- installed record's +0x0C HP and +0x18 INT, so the copy (and the
-- ctx[+0x287] INT boost it applies) can be read off directly.
--
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_summon_body_stats.lua \
--     --sstate <gimard_summon_start state> --frames 2400

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)

local FN_ROLL   = 0x801DD0AC
local CTX_PTR   = 0x8007BD24
local ACTORS    = 0x801C9370 -- live actor ptr table, +slot*4
local BODY_SLOT = 0x801C9358 -- slot-7 cast body record

local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function in_ram(a) return a >= 0x80000000 and a < 0x80800000 end

local g_elapsed = 0
local hits = 0
local csv = probe.csv_open(probe.out_path("summon_body_stats.csv"),
    "vsync,seat,move_id,defender,ctx287,actor_hp,actor_int,actor_udf,actor_ldf,rec_hp,rec_int,rec_udf,rec_ldf")

local function arm_all()
    probe.arm_breakpoint(FN_ROLL, "Exec", 4, "dmg_roll", function()
        local r = PCSX.getRegisters()
        local a1 = tou32(r.GPR.n.a1) % 0x100
        local a2 = tou32(r.GPR.n.a2) % 0x100
        if a1 ~= 7 then return end
        hits = hits + 1
        local c = u32(CTX_PTR)
        local seat, move_id, f287 = 0xFF, 0xFF, 0xFF
        if in_ram(c) then
            seat = u8(c + 0x13)
            f287 = u8(c + 0x287)
            local actor = u32(ACTORS + seat * 4)
            if in_ram(actor) then move_id = u8(actor + 0x1DF) end
        end
        local body = u32(ACTORS + 7 * 4)
        local rec = u32(BODY_SLOT)
        local ahp, aint, audf, aldf = 0, 0, 0, 0
        if in_ram(body) then
            ahp, aint = u16(body + 0x14C), u16(body + 0x168)
            audf, aldf = u16(body + 0x15C), u16(body + 0x160)
        end
        local rhp, rint, rudf, rldf = 0, 0, 0, 0
        if in_ram(rec) then
            rhp, rint = u16(rec + 0x0C), u16(rec + 0x18)
            rudf, rldf = u16(rec + 0x14), u16(rec + 0x16)
        end
        csv:row("%d,%d,0x%02X,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
            g_elapsed, seat, move_id, a2, f287, ahp, aint, audf, aldf, rhp, rint, rudf, rldf)
        PCSX.log(string.format(
            "[summon-body] vsync=%d move=0x%02X def=%d ctx287=%d actor hp=%d int=%d | record hp=%d int=%d",
            g_elapsed, move_id, a2, f287, ahp, aint, rhp, rint))
    end)
    return { "roll" }
end

probe.run{
    sstate         = SSTATE,
    capture_frames = FRAMES,
    boot_delay     = 60,
    on_arm         = arm_all,
    on_capture     = function(_, v) g_elapsed = v end,
    on_summary     = function()
        PCSX.log(string.format("[summon-body] slot-7 rolls: %d", hits))
        csv:close()
    end,
}
