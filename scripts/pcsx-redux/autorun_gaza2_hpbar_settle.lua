-- autorun_gaza2_hpbar_settle.lua
--
-- Why state 0x51 of the battle-action SM (FUN_801E295C) can park forever, and
-- the endless battle-camera orbit that follows.
--
-- The park was already localised: ctx+7 = 0x51 (done / fade-down), the exit
-- gate `ctx+0x6D8 < 0 && ctx+0x276 == 0` has +0x276 == 0, and +0x6D8 sits at
-- exactly the 0x3C that state 0x50 seeded, never decremented, while the
-- scratchpad frame delta DAT_1F800393 stays healthy. So the arm runs but the
-- countdown is skipped.
--
-- The skip is structural, straight out of the 0x51 arm's disassembly:
--
--   801e6044  jal  0x801e7250        ; the HP-bar SETTLE check
--   801e604c  bne  v0,zero,0x801e60b8 ; not settled -> jump PAST the decrement
--   801e6054  lh   v0,0x2(s7)         ; s7+2 == ctx+0x6D8
--   801e6068  lbu  v0,0x393(v0)       ; DAT_1F800393 frame delta
--   801e6070  subu a0,v1,v0
--   801e6074  sh   a0,0x2(s7)         ; ctx+0x6D8 -= dt   <- THE decrement
--
-- and FUN_801E7250 (52 instructions) returns 1 - "not settled" - when the
-- ACTING actor's target field decides a party-side HP bar has not caught up:
--
--   a0 = ACTORS[ctx+0x13] + 0x1DD          ; the action's target slot
--   a0 in 3..7  -> return 0                 ; a monster target never blocks
--   a0 in 0..2  -> return ACTORS[a0][+0x14C] != ACTORS[a0][+0x172]
--   a0 == 8     -> return 1 if ANY slot i < ctx[0] has +0x14C != +0x172
--   a0 >  8     -> return 0
--
-- +0x14C is live HP, +0x172 is the value the HP bar currently DISPLAYS. They
-- converge through a separate drain: actor+0x10 is a pending-delta
-- accumulator, and the per-actor tick FUN_80047430 moves a quarter of it into
-- +0x172 every frame (`0x172 -= (acc+3)/4`, `acc -= (acc+3)/4`) but ONLY when
-- acc != 0 (`lw a0,0x10(s2); beq a0,zero,<skip>` at 0x800474E8). So a slot
-- whose bar and HP disagree while its accumulator reads zero has NO mechanism
-- left to converge, FUN_801E7250 answers 1 for the rest of the battle, the
-- 0x51 countdown never moves, and the idle camera sweep in FUN_801D0748 keeps
-- orbiting - the community symptom exactly.
--
-- This probe does two things.
--
-- 1. MEASURE. It logs, per vsync, every slot's (+0x14C, +0x172, +0x10) triple
--    alongside ctx+7 / ctx+0x6D8, and arms three Exec breakpoints:
--      * FUN_801E295C entry            - is the SM even entered this frame?
--      * FUN_801E7250 entry            - is the settle check reached?
--      * 0x801E604C (the jal's return) - v0 IS the settle verdict.
--    Those three answer "which arm runs and where does it return before the
--    0x51 body" without guessing.
--
-- 2. FALSIFY OR CONFIRM. LEGAIA_DESYNC_SLOT injects the hypothesised state
--    directly: offset one party slot's displayed HP +0x172 by
--    LEGAIA_DESYNC_DELTA and clear its accumulator +0x10. If the mechanism
--    above is the whole story, the next action that targets the party side
--    (an enemy cast on the party, or a party heal) must park at 0x51 forever
--    with the camera still orbiting. A run that does NOT park refutes it.
--
-- Outputs (under captures/gaza2_hpbar_settle/<ts>/):
--   timeline.csv      change-triggered per-vsync SM + HP/bar/accumulator table
--   settle.csv        one row per 0x51 settle check: verdict + who blocked it
--   wedge.txt         full diagnostic the first time 0x51 parks
--   summary.txt       terse verdict
--
-- Knobs (env):
--   LEGAIA_FRAMES         capture vsyncs (default 3000)
--   LEGAIA_STALL_N        vsyncs at an unchanged ctx+7 inside 0x50..0x52 that
--                         counts as a park (default 600; a healthy 0x51 is
--                         well under 100)
--   LEGAIA_DESYNC_SLOT    party slot 0..2 to desync, or -1 for observe-only
--   LEGAIA_DESYNC_DELTA   signed offset written into +0x172 (default 1)
--   LEGAIA_DESYNC_AT      vsync at which to inject (default 240)
--   LEGAIA_AUTOPILOT      press the next macro button every N vsyncs
--   LEGAIA_AUTOPILOT_SEQ  comma-separated button cycle
--   LEGAIA_GODMODE        1 = top up party HP each frame (an intervention;
--                         see the note by the godmode block below)
--
-- Run:
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_gaza2_hpbar_settle.lua \
--     --scenario battle_gaza2_prompt --frames 3000
--
-- Lua breakpoints need -interpreter -debugger, so never launch this --fast.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad   = require("probe.pad")

local SSTATE  = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES  = probe.getenv_num("LEGAIA_FRAMES", 3000)
local STALL_N = probe.getenv_num("LEGAIA_STALL_N", 600)
local DESYNC_SLOT  = probe.getenv_num("LEGAIA_DESYNC_SLOT", -1)
local DESYNC_DELTA = probe.getenv_num("LEGAIA_DESYNC_DELTA", 1)
local DESYNC_AT    = probe.getenv_num("LEGAIA_DESYNC_AT", 240)
local GODMODE   = probe.getenv_num("LEGAIA_GODMODE", 0)
local AUTOPILOT = probe.getenv_num("LEGAIA_AUTOPILOT", 0)
local AUTOPILOT_SEQ = probe.getenv("LEGAIA_AUTOPILOT_SEQ",
    "CROSS,RIGHT,CROSS,DOWN,CROSS,CROSS,RIGHT,CROSS,DOWN,DOWN,CROSS,CROSS,CROSS,CROSS")

if probe.getenv("LEGAIA_CORE", ""):match("^dynarec$") or probe.getenv("LEGAIA_CORE", ""):match("^interpreter%-nodebug$") then
    PCSX.log("[settle] REFUSING --fast/--timing launch: Lua breakpoints need the debugger hook")
    PCSX.quit(3)
    return
end

local CTX_PTR = 0x8007BD24
local ACTORS  = 0x801C9370
local CAM_YAW = 0x8007B792
local BAND_TIMER_OFF = 0x6D8
local FRAME_DT = 0x1F800393

-- The three instrumentation points, all read off the 0x51 arm's disassembly.
local SM_ENTRY   = 0x801E295C   -- FUN_801E295C, the battle-action SM
local SETTLE_FN  = 0x801E7250   -- the HP-bar settle check
local SETTLE_RET = 0x801E604C   -- `bne v0,zero,...` - v0 is the verdict here

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function i16(a) local v = u16(a); return v >= 0x8000 and v - 0x10000 or v end
local function i32(a) local v = u32(a); return v >= 0x80000000 and v - 0x100000000 or v end
local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end

local function actor_of(seat)
    local a = u32(ACTORS + seat * 4)
    return in_ram(a) and a or 0
end

------------------------------------------------------------------
local auto_seq = {}
if AUTOPILOT > 0 then
    for name in AUTOPILOT_SEQ:gmatch("[^,]+") do
        local btn = pad.BTN[name:upper():gsub("%s", "")]
        if btn then auto_seq[#auto_seq + 1] = { btn = btn, name = name:upper() } end
    end
end
local auto_i = 1
local pad_release_at, pad_btn_held = nil, nil

local g_elapsed = 0
local armed = false
local last_key = ""
local last_ctx7, ctx7_since = -1, 0
local park_yaw_moves, park_yaw_last = 0, -1
local wedge_dumped = false
local band_seen, band_max_dwell = {}, {}
local injected = false

-- Breakpoint counters, reset every vsync so the timeline can say "the SM ran
-- N times this frame and the settle check ran M times".
local sm_hits, settle_hits, settle_ret_hits = 0, 0, 0
local sm_total, settle_total = 0, 0
local last_verdict = -1
local verdict_runs = { [0] = 0, [1] = 0 }

local timeline = probe.csv_open(probe.out_path("timeline.csv"),
    "vsync,ctx7,seat,target_1dd,band_timer,frame_dt,c276,c249,c24d,cam_yaw," ..
    "sm_hits,settle_hits,verdict," ..
    "hp0,bar0,acc0,hp1,bar1,acc1,hp2,bar2,acc2,hp3,bar3,acc3")

local settle_csv = probe.csv_open(probe.out_path("settle.csv"),
    "vsync,ctx7,seat,target_1dd,verdict,band_timer,blocker")

-- Reproduce FUN_801E7250 in Lua, from its disassembly, so the probe can NAME
-- the slot that answers "not settled" instead of inferring it.
local function settle_check(c)
    local seat = u8(c + 0x13)
    local actor = actor_of(seat)
    if actor == 0 then return 0, "no acting actor" end
    local tgt = u8(actor + 0x1DD)
    if tgt < 8 then
        if tgt > 2 then return 0, "monster target (slot >2): never blocks" end
        local t = actor_of(tgt)
        if t == 0 then return 0, "target slot empty" end
        local hp, bar = u16(t + 0x14C), u16(t + 0x172)
        if hp ~= bar then
            return 1, string.format("slot %d hp=%d bar=%d acc=%d", tgt, hp, bar,
                i32(t + 0x10))
        end
        return 0, "single target settled"
    elseif tgt == 8 then
        local n = u8(c + 0x00)
        for s = 0, n - 1 do
            local t = actor_of(s)
            if t ~= 0 then
                local hp, bar = u16(t + 0x14C), u16(t + 0x172)
                if hp ~= bar then
                    return 1, string.format("slot %d hp=%d bar=%d acc=%d", s, hp,
                        bar, i32(t + 0x10))
                end
            end
        end
        return 0, string.format("all-target settled over %d slot(s)", n)
    end
    return 0, string.format("target 0x%02X > 8: never blocks", tgt)
end

local function arm_bps()
    local c = u32(CTX_PTR)
    if not in_ram(c) then return false end
    probe.arm_breakpoint(SM_ENTRY, "Exec", 4, "sm_entry", function()
        sm_hits = sm_hits + 1; sm_total = sm_total + 1
    end)
    probe.arm_breakpoint(SETTLE_FN, "Exec", 4, "settle_fn", function()
        settle_hits = settle_hits + 1; settle_total = settle_total + 1
    end)
    -- The decisive one: this is the delay-slot-follower of `jal 0x801e7250`,
    -- so v0 here IS the settle verdict, and the very next instruction
    -- (`bne v0,zero,0x801e60b8`) is what skips the ctx+0x6D8 decrement.
    probe.arm_breakpoint(SETTLE_RET, "Exec", 4, "settle_ret", function()
        local r = PCSX.getRegisters()
        local v0 = tonumber(r.GPR.n.v0) or 0
        local verdict = (v0 ~= 0) and 1 or 0
        settle_ret_hits = settle_ret_hits + 1
        verdict_runs[verdict] = verdict_runs[verdict] + 1
        local cc = u32(CTX_PTR)
        if not in_ram(cc) then return end
        if verdict ~= last_verdict then
            last_verdict = verdict
            local _, why = settle_check(cc)
            local seat = u8(cc + 0x13)
            local actor = actor_of(seat)
            settle_csv:row("%d,0x%02X,%d,0x%02X,%d,%d,%s",
                g_elapsed, u8(cc + 7), seat,
                actor ~= 0 and u8(actor + 0x1DD) or 255,
                verdict, i16(cc + BAND_TIMER_OFF), why)
        end
    end)
    PCSX.log(string.format("[settle] armed on ctx=0x%08X", c))
    return true
end

local function dump_wedge(c, why)
    local lines = {}
    local function add(f, ...) lines[#lines + 1] = string.format(f, ...) end
    add("=== gaza2 0x51 park: %s ===", why)
    add("vsync=%d  ctx=0x%08X  ctx+7=0x%02X (unchanged %d vsyncs)",
        g_elapsed, c, u8(c + 7), ctx7_since)
    add("acting seat ctx+0x13 = %d", u8(c + 0x13))
    add("camera yaw _DAT_8007B792 = 0x%04X, changed %d time(s) during this park",
        u16(CAM_YAW), park_yaw_moves)
    add("")
    add("-- the 0x51 exit gate: ctx+0x6D8 < 0 AND ctx+0x276 == 0 --")
    add("ctx+0x6D8 band timer = %d   (state 0x50 seeds 0x3C = 60)",
        i16(c + BAND_TIMER_OFF))
    add("ctx+0x276            = %d", u8(c + 0x276))
    add("DAT_1F800393 frame dt = %d", probe.mem.read_scratch_u8(FRAME_DT))
    add("")
    add("-- the decrement's own gate: FUN_801E7250 at 0x801E6044 --")
    local verdict, why2 = settle_check(c)
    add("recomputed verdict = %d  (%s)", verdict, why2)
    add("live breakpoint verdict runs: settled=%d not-settled=%d",
        verdict_runs[0], verdict_runs[1])
    add("SM entries so far = %d, settle-check entries = %d, jal returns = %d",
        sm_total, settle_total, settle_ret_hits)
    if verdict ~= 0 then
        add("-> v0 != 0 at 0x801E604C, so `bne v0,zero,0x801E60B8` jumps PAST")
        add("   the `sh a0,0x2(s7)` at 0x801E6074. ctx+0x6D8 can never move.")
    end
    add("")
    add("-- per-slot HP vs displayed HP vs pending-drain accumulator --")
    add("  seat  actor       +0x14C hp  +0x172 bar  +0x10 acc  +0x1DD tgt  +0x1D9  +0x4")
    for seat = 0, 7 do
        local a = actor_of(seat)
        if a ~= 0 then
            add("  %4d  0x%08X  %9d  %10d  %9d  %10d  0x%02X    0x%08X",
                seat, a, u16(a + 0x14C), u16(a + 0x172), i32(a + 0x10),
                u8(a + 0x1DD), u8(a + 0x1D9), u32(a + 0x4))
        end
    end
    add("")
    add("ctx+0x00 (party count used by the all-target loop) = %d", u8(c + 0x00))
    add("ctx+0x249 = %d   ctx+0x24D = %d", u8(c + 0x249), u8(c + 0x24D))
    probe.write_snapshot(probe.out_path("wedge.txt"), table.concat(lines, "\n"))
    for _, l in ipairs(lines) do PCSX.log("[settle] " .. l) end
end

probe.run{
    sstate         = SSTATE,
    capture_frames = FRAMES,
    boot_delay     = 60,
    on_arm         = function() return { "deferred" } end,
    on_capture     = function(ctx, v)
        g_elapsed = v
        if not armed and v >= 2 then armed = arm_bps() end

        if pad_release_at and v >= pad_release_at then
            pad.release(pad_btn_held)
            pad_release_at, pad_btn_held = nil, nil
        end
        if AUTOPILOT > 0 and #auto_seq > 0 and v % AUTOPILOT == 0 then
            local e = auto_seq[auto_i]
            auto_i = (auto_i % #auto_seq) + 1
            if pad_btn_held then pad.release(pad_btn_held) end
            pad.force(e.btn)
            pad_btn_held, pad_release_at = e.btn, v + 4
        end

        local c = u32(CTX_PTR)
        if not in_ram(c) then return end

        -- Godmode is an INTERVENTION and is off by default here. Note it also
        -- writes +0x172, so it papers over exactly the desync this probe is
        -- looking for; a run that needs the party kept alive should prefer the
        -- desync injection instead of leaning on this.
        if GODMODE ~= 0 then
            for s = 0, 2 do
                local a = actor_of(s)
                if a ~= 0 then
                    local cur, max = u16(a + 0x14C), u16(a + 0x14E)
                    if cur > 0 and max > 0 and cur < max then
                        probe.write_u16(a + 0x14C, max)
                        probe.write_u16(a + 0x172, max)
                    end
                end
            end
        end

        -- The falsification lever: put the hypothesised desync in by hand.
        -- +0x172 is moved off +0x14C and the accumulator +0x10 is cleared, so
        -- FUN_80047430's quarter-step drain (which only runs when acc != 0)
        -- has nothing left to converge with.
        if not injected and DESYNC_SLOT >= 0 and v >= DESYNC_AT then
            -- Slot 8 mirrors the game's own "all party slots" convention: the
            -- all-target arm of FUN_801E7250 blocks on ANY unsettled party
            -- slot, so desyncing all three is the strongest form of the test.
            local first = (DESYNC_SLOT == 8) and 0 or DESYNC_SLOT
            local last  = (DESYNC_SLOT == 8) and 2 or DESYNC_SLOT
            for s = first, last do
                local a = actor_of(s)
                if a ~= 0 then
                    local hp = u16(a + 0x14C)
                    local want = (hp + DESYNC_DELTA) % 0x10000
                    probe.write_u16(a + 0x172, want)
                    probe.write_u16(a + 0x10, 0)
                    probe.write_u16(a + 0x12, 0)
                    injected = true
                    PCSX.log(string.format(
                        "[settle] INJECT vsync=%d slot=%d hp=%d -> bar=%d, acc cleared",
                        v, s, hp, want))
                end
            end
        end

        local ctx7 = u8(c + 7)
        local yaw = u16(CAM_YAW)
        if ctx7 ~= last_ctx7 then
            if last_ctx7 >= 0 and ctx7_since > (band_max_dwell[last_ctx7] or -1) then
                band_max_dwell[last_ctx7] = ctx7_since
            end
            last_ctx7, ctx7_since = ctx7, 0
            park_yaw_moves, park_yaw_last = 0, yaw
            band_seen[ctx7] = (band_seen[ctx7] or 0) + 1
        else
            if yaw ~= park_yaw_last then
                park_yaw_moves = park_yaw_moves + 1
                park_yaw_last = yaw
            end
            ctx7_since = ctx7_since + 1
            if ctx7_since > (band_max_dwell[ctx7] or -1) then
                band_max_dwell[ctx7] = ctx7_since
            end
        end

        local seat = u8(c + 0x13)
        local actor = actor_of(seat)
        local cells = {}
        for s = 0, 3 do
            local a = actor_of(s)
            cells[#cells + 1] = string.format("%d,%d,%d",
                a ~= 0 and u16(a + 0x14C) or 0,
                a ~= 0 and u16(a + 0x172) or 0,
                a ~= 0 and i32(a + 0x10) or 0)
        end
        local body = table.concat(cells, ",")
        local key = string.format("0x%02X,%d,0x%02X,%d,%d,%d,%d,%d",
            ctx7, seat, actor ~= 0 and u8(actor + 0x1DD) or 255,
            i16(c + BAND_TIMER_OFF), probe.mem.read_scratch_u8(FRAME_DT),
            u8(c + 0x276), u8(c + 0x249), u8(c + 0x24D)) .. "|" .. body
        if key ~= last_key then
            last_key = key
            timeline:row("%d,0x%02X,%d,0x%02X,%d,%d,%d,%d,%d,0x%04X,%d,%d,%d,%s",
                v, ctx7, seat, actor ~= 0 and u8(actor + 0x1DD) or 255,
                i16(c + BAND_TIMER_OFF), probe.mem.read_scratch_u8(FRAME_DT),
                u8(c + 0x276), u8(c + 0x249), u8(c + 0x24D), yaw,
                sm_hits, settle_hits, last_verdict, body)
        end
        sm_hits, settle_hits = 0, 0

        -- 0x50..0x52 is the done/cleanup band; a healthy pass through it is
        -- well under 100 vsyncs, so a park here is the wedge.
        if not wedge_dumped and ctx7_since >= STALL_N
                and ctx7 >= 0x50 and ctx7 <= 0x52 then
            wedge_dumped = true
            dump_wedge(c, string.format("ctx+7=0x%02X parked >= %d vsyncs",
                ctx7, STALL_N))
        end
        if wedge_dumped and ctx7_since >= STALL_N * 3 then
            ctx.request_quit = true
        end
    end,
    on_summary     = function()
        local c = u32(CTX_PTR)
        local lines = {}
        local function add(f, ...) lines[#lines + 1] = string.format(f, ...) end
        add("gaza2 hp-bar settle run summary")
        add("capture vsyncs: %d", g_elapsed)
        add("desync injected: %s (slot %d, delta %d, at vsync %d)",
            tostring(injected), DESYNC_SLOT, DESYNC_DELTA, DESYNC_AT)
        add("SM entries: %d   settle-check entries: %d   jal returns: %d",
            sm_total, settle_total, settle_ret_hits)
        add("settle verdicts: settled=%d not-settled=%d",
            verdict_runs[0], verdict_runs[1])
        add("wedge dumped: %s", tostring(wedge_dumped))
        if in_ram(c) then
            add("final ctx+7 = 0x%02X (unchanged %d vsyncs), ctx+0x6D8 = %d",
                u8(c + 7), ctx7_since, i16(c + BAND_TIMER_OFF))
        end
        local seen = {}
        for st in pairs(band_seen) do seen[#seen + 1] = st end
        table.sort(seen)
        local parts, dparts = {}, {}
        for _, st in ipairs(seen) do
            parts[#parts + 1] = string.format("0x%02X x%d", st, band_seen[st])
            dparts[#dparts + 1] = string.format("0x%02X=%d", st, band_max_dwell[st] or 0)
        end
        add("ctx+7 states visited: %s", table.concat(parts, ", "))
        add("ctx+7 max dwell: %s", table.concat(dparts, ", "))
        add("done-band max dwell: %d", math.max(
            band_max_dwell[0x50] or 0, band_max_dwell[0x51] or 0,
            band_max_dwell[0x52] or 0))
        local text = table.concat(lines, "\n")
        probe.write_snapshot(probe.out_path("summary.txt"), text)
        for _, l in ipairs(lines) do PCSX.log("[settle] " .. l) end
        timeline:close()
        settle_csv:close()
    end,
}
