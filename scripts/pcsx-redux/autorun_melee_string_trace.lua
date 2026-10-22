-- autorun_melee_string_trace.lua
--
-- Per-frame trace of a party melee string against one monster, for the
-- approach / separation residual the retail comparison measures on
-- `battle_melee_hit_spark` (docs/subsystems/battle-action.md, "Where an
-- action leaves its combatants"). From a catalogued mid-fight battle state
-- it runs with no input and writes:
--
--   frames.csv  one row per vsync: the main loop's frame counter, the frame
--               step *(0x1F800393), the action state ctx[+0x07], the acting
--               seat ctx[+0x13], and for the attacker and its target the
--               live pair +0x34/+0x38, the body pair +0x3C/+0x40, the
--               facing +0x46, the committed clip +0x1D9, the queued clip
--               +0x1DA, the flag byte +0x1DC and the anim cursor node+0x68
--   range.csv   every FUN_8004E2F0 return (0x8004E560): vsync, a0, a1, v0
--               (0 = in range) and the return address, so the anim tick's
--               two polls (back-off 0x80047C40, root motion 0x80047D50)
--               read apart from the action SM's
--   sep.csv     every FUN_80050BB8 push (the not-taken branch at
--               0x80050CD0): vsync, the two actor pointers' slots, and both
--               body pairs at the push
--
-- These are retail RAM values: never commit the outputs.
--
-- Run (interpreter; breakpoints):
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_melee_string_trace.lua \
--     --scenario rim_elm_gimard_victory --frames 600 --iso <scratch copy>
--
-- Env: LEGAIA_ATTACKER (0), LEGAIA_TARGET (3) - the two actor-table seats
-- the per-frame rows record. LEGAIA_QUEUE (hex bytes, e.g. "0d0f0e"): when
-- set, the probe taps CROSS every LEGAIA_PRESS_PERIOD (60) vsyncs until the
-- attacker's action reaches state 0x0A, then writes an Attack (`+0x1DE = 3`)
-- with that swing queue at `+0x1DF` and the target at `+0x1DD` - the
-- injection autorun_cast_sequence.lua uses for a cast - and records from
-- there; rows before the inject are skipped. LEGAIA_TARGET_HP (0): when
-- non-zero, the target's HP words +0x14C / +0x172 are held at it until the
-- inject, for a state whose own action would end the fight.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 600)
local ATTACKER = probe.getenv_num("LEGAIA_ATTACKER", 0)
local TARGET   = probe.getenv_num("LEGAIA_TARGET", 3)
local QUEUE    = probe.getenv("LEGAIA_QUEUE", "")
local PRESS_PERIOD = probe.getenv_num("LEGAIA_PRESS_PERIOD", 60)
local TARGET_HP = probe.getenv_num("LEGAIA_TARGET_HP", 0)
local injected = (QUEUE == "")
local last_flow = -1

local CTX_PTR       = 0x8007BD24
local ACTORS        = 0x801C9370
local FRAME_COUNTER = 0x8007B798
local RANGE_ENTRY   = 0x8004E2F0
local RANGE_RET     = 0x8004E560
local SEP_PUSH      = 0x80050CD0

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function i16(a) local v = u16(a); return v >= 0x8000 and v - 0x10000 or v end
local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end
local function actor(seat)
    local a = u32(ACTORS + seat * 4)
    return in_ram(a) and a or nil
end
local function slot_of(ptr)
    for s = 0, 7 do
        if u32(ACTORS + s * 4) == ptr then return s end
    end
    return -1
end

local vsync = 0
local frames = probe.csv_open(probe.out_path("frames.csv"),
    "vsync,frame,step,state,acting,"
    .. "a_x,a_z,a_bx,a_bz,a_face,a_clip,a_queued,a_1dc,a_cursor,"
    .. "t_x,t_z,t_bx,t_bz,t_face,t_clip,t_queued,t_1dc,t_cursor")
local range = probe.csv_open(probe.out_path("range.csv"), "vsync,a0,a1,v0,ra")
local sep = probe.csv_open(probe.out_path("sep.csv"),
    "vsync,s1,s2,s1_bx,s1_bz,s2_bx,s2_bz,s1_x,s1_z,s2_x,s2_z")
local last = { a0 = -1, a1 = -1, ra = 0 }
local pushes = 0

local function actor_cols(p)
    if not p then return "-1,-1,-1,-1,-1,-1,-1,-1,-1" end
    local node = u32(p + 0x22C)
    local cursor = in_ram(node) and u16(node + 0x68) or -1
    return string.format("%d,%d,%d,%d,%d,%d,%d,%d,%d",
        i16(p + 0x34), i16(p + 0x38), i16(p + 0x3C), i16(p + 0x40),
        u16(p + 0x46) % 0x1000, u8(p + 0x1D9), u8(p + 0x1DA), u8(p + 0x1DC), cursor)
end

probe.run{
    sstate         = SSTATE,
    capture_frames = FRAMES,
    boot_delay     = 60,
    on_arm = function()
        probe.arm_breakpoint(RANGE_ENTRY, "Exec", 4, "range_entry", function()
            local r = PCSX.getRegisters()
            last.a0 = tonumber(r.GPR.n.a0) % 0x100
            last.a1 = tonumber(r.GPR.n.a1) % 0x100
            last.ra = tonumber(r.GPR.n.ra) or 0
        end)
        probe.arm_breakpoint(RANGE_RET, "Exec", 4, "range_ret", function()
            local r = PCSX.getRegisters()
            range:row("%d,%d,%d,%d,0x%08X", vsync, last.a0, last.a1,
                tonumber(r.GPR.n.v0) or -1, last.ra)
        end)
        probe.arm_breakpoint(SEP_PUSH, "Exec", 4, "sep_push", function()
            local r = PCSX.getRegisters()
            local s1 = tonumber(r.GPR.n.s1) or 0
            local s2 = tonumber(r.GPR.n.s2) or 0
            pushes = pushes + 1
            sep:row("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d", vsync, slot_of(s1), slot_of(s2),
                i16(s1 + 0x3C), i16(s1 + 0x40), i16(s2 + 0x3C), i16(s2 + 0x40),
                i16(s1 + 0x34), i16(s1 + 0x38), i16(s2 + 0x34), i16(s2 + 0x38))
        end)
        return {}
    end,
    on_capture = function(_, v)
        vsync = v
        local c = u32(CTX_PTR)
        local st, acting = -1, -1
        if in_ram(c) then
            st = u8(c + 0x07)
            acting = u8(c + 0x13)
        end
        probe.pad_release(probe.BTN.CROSS)
        if not injected and TARGET_HP > 0 then
            -- Keep the target standing through whatever ends the state's own
            -- action (a capture taken mid-kill), so the fight reaches the
            -- attacker's next turn.
            local t = actor(TARGET)
            if t then
                probe.write_u16(t + 0x14C, TARGET_HP)
                probe.write_u16(t + 0x172, TARGET_HP)
            end
        end
        if not injected then
            local flow = (in_ram(c) and u8(c + 6) or -1) * 65536 + st * 256 + acting
            if flow ~= last_flow then
                PCSX.log(string.format("[melee] flow t%d ctx6=0x%02X ctx7=0x%02X acting=%d",
                    v, in_ram(c) and u8(c + 6) or -1, st, acting))
                last_flow = flow
            end
            local sub = v % PRESS_PERIOD
            if sub >= PRESS_PERIOD / 2 and sub < PRESS_PERIOD / 2 + 4 then
                probe.pad_force(probe.BTN.CROSS)
            end
            local a = actor(ATTACKER)
            if st == 0x0A and acting == ATTACKER and a then
                probe.write_u8(a + 0x1DE, 3)
                probe.write_u8(a + 0x1DD, TARGET)
                for i = 0, 15 do
                    local hex = QUEUE:sub(i * 2 + 1, i * 2 + 2)
                    probe.write_u8(a + 0x1DF + i, hex ~= "" and tonumber(hex, 16) or 0)
                end
                injected = true
                PCSX.log(string.format("[melee] inject t%d queue=%s", v, QUEUE))
            else
                return
            end
        end
        frames:row("%d,%d,%d,%d,%d,%s,%s", v, u32(FRAME_COUNTER),
            probe.read_scratch_u8(0x1F800393), st, acting,
            actor_cols(actor(ATTACKER)), actor_cols(actor(TARGET)))
    end,
    on_done = function()
        local line = string.format(
            "=== melee string trace: %d vsyncs, %d separation pushes ===", vsync, pushes)
        probe.write_snapshot(probe.out_path("summary.txt"), line)
        PCSX.log("[melee] " .. line)
    end,
    on_summary = function() end,
}
