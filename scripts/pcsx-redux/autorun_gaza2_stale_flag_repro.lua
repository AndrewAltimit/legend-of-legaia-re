-- autorun_gaza2_stale_flag_repro.lua
--
-- Close the "which anim-driver field does summon staging leave stale?"
-- sub-question of the 0x19 attack-approach park (scenario
-- battle_gaza2_park_0x19_summon_melee).
--
-- Static candidate (from the dumps): actor +0x1DC bit 2 (mask 0x4), the
-- "stage idle at clip end" anim event flag.
--   * The damage primitive FUN_800402F4 stages a surviving target's light
--     flinch with `+0x1DC |= 4` (exit-to-idle) + `|= 1` (commit now)
--     (80042138..80042170) - this is what plays Gaza's reaction clip 2/2
--     during the summon's staging round-trip.
--   * The anim tick FUN_80047430 has TWO commit sites: the mid-clip event
--     path clears only bits 0-1 (`andi 0xFC`, 80047a38) and so PRESERVES
--     bit 2; the natural-end path (phase >= stream frame count) tests
--     `+0x1DC & 4` and, if set, clobbers the queued clip with idle
--     (`sb zero,0x1da` at 0x80047B44) before clearing bits 0-2
--     (`andi 0xF8`, 80047b50) and committing.
--   * The state-0x14 walk-less fallback stages the Move clip via
--     `sb v0,0x1da(s3)` (801e32b0) + `+0x1DC |= 1` (801e32d4/801e35c8) -
--     bit 1 forces the event-path commit, which is exactly the path that
--     preserves a stale bit 2.
--   So: summon damage arms flinch with bit 2; Gaza's melee stages the Move
--   clip before the flinch's natural end consumes the bit; the event-path
--   commit engages Move with bit 2 still set; Move's FIRST natural end
--   (5-frame stream, rate 2, speed scale 8 => ~10-12 vsyncs) stages idle =>
--   pair 0/0, drive dead (idle entry +0xC == 0), state 0x19 re-polls forever.
--
-- This probe proves the terminal half LIVE on the parked save, causally:
--   control    (LEGAIA_STALE=0): poke ctx state -> 0x14 (the approach-fix
--     bounce). Retail re-stages the Move clip with +0x1DC bit 2 CLEAR: the
--     clip must loop across natural-end recommits (pair stays 1/1), the
--     boss walks in ~20 units/vsync, range hits 0, state reaches 0x1E.
--   experiment (LEGAIA_STALE=1): identical bounce but first re-arm the
--     stale flag (+0x1DC |= 4), reconstructing what the summon staging
--     leaves behind. The same re-staged clip must die at its FIRST natural
--     end: a `sb zero,0x1da` write from PC 0x80047B44, pair 0/0, position
--     frozen beyond reach, state parked in 0x19 - the exact caught-park
--     signature (the park save itself shows +0x1DC == 0 because the killing
--     end-path commit consumed the bit via `andi 0xF8`).
-- Write-watchpoints on +0x1DA/+0x1D9/+0x1DC record every writer PC, so the
-- verdict is grounded in which code wrote what, not in correlation.
--
-- Run (interpreter+debugger - write BPs must fire; ~8 min wall clock each):
--   timeout -k 10 900 bash scripts/pcsx-redux/run_probe.sh --isolate-config \
--     --sstate saves/library/pcsx-redux/98022a6d3a4db2a2753b5b29214fcf75de9279e8a8b1ee8a7d91bdf180acead1.sstate \
--     --lua scripts/pcsx-redux/autorun_gaza2_stale_flag_repro.lua \
--     --frames 1500
--   LEGAIA_STALE=0 for the control run, =1 (default) for the repro run.
--
-- Output (LEGAIA_OUT_DIR or captures/gaza2_stale_flag_repro/<ts>/):
--   anim.csv     per-vsync driver state: state byte, anim pair, +0x1DC,
--                node phase (+0x68), current entry index, loop-hold +0x176,
--                speed +0x21D, Gaza + target positions
--   writers.csv  every write to +0x1DA/+0x1D9/+0x1DC: tick,label,addr,pc,ra,
--                prev_value (pre-store, per probe.watch contract)
--   summary.txt  verdict + timeline

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local STALE = probe.getenv_num("LEGAIA_STALE", 1) ~= 0
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1500)
local POKE_V = probe.getenv_num("LEGAIA_POKE_VSYNC", 60)

local CTX_PTR = 0x8007BD24
local ACTORS = 0x801C9370
local MONREC = 0x801C9348
local TICK_FN = 0x80047430 -- FUN_80047430 (anim-node tick; a0 = node)
local KILL_PC = 0x80047B44 -- the tick's end-path `sb zero,0x1da(s2)` idle clobber

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function i16(a) local v = u16(a); return v >= 0x8000 and v - 0x10000 or v end
local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end

local vsync = 0
local gaza, node = nil, nil
local entries = {} -- entry ptr -> index, from the seat-3 record's +0x4C array
local poked = false
local engaged_v, killed_v, arrived_v = nil, nil, nil
local frozen_run, last_gx, last_gz = 0, nil, nil
local last_sig = ""

local anim_csv = probe.csv_open(probe.out_path("anim.csv"),
    "vsync,ctx7,a3_1da,a3_1d9,a3_1dc,phase,entry_idx,hold176,spd21d,gx,gz,tx,tz")
local writers_csv = probe.csv_open(probe.out_path("writers.csv"),
    "tick,label,addr,pc,ra,prev_value")
local writers = probe.watch.new{
    csv = writers_csv,
    detail_path = probe.out_path("writers.detail.txt"),
    max_detail = 12,
    elapsed = function() return vsync end,
}

local function on_tick_entry()
    -- Discover seat 3's anim node: at FUN_80047430 entry a0 = node and
    -- node+0x5A is the actor slot. One hit is enough.
    if node then return end
    local r = PCSX.getRegisters()
    local a0 = tonumber(r.GPR.n.a0) % 0x100000000 -- NOT bit.band: signed under LuaJIT
    if in_ram(a0) and u16(a0 + 0x5A) == 3 then
        node = a0
        PCSX.log(string.format("[stale] seat-3 anim node = 0x%08X (entry=0x%08X phase=0x%04X)",
            node, u32(node + 0x4C), u16(node + 0x68)))
    end
end

-- +0x1DA gets its own callback (NOT via probe.watch): PCSX honours one
-- breakpoint per (addr, kind), so the kill-site detection must live in the
-- same callback that logs the writer row. Mirrors the probe.watch row
-- format; the logged value is the PRE-store byte, per the watch contract.
local function on_1da_write()
    -- NB: bit.band() returns a SIGNED 32-bit value under LuaJIT, so an
    -- 0x80xxxxxx PC comes back negative and never compares equal to a
    -- positive Lua constant (and %08X prints it 0xFFFFFFFF80xxxxxx).
    -- Normalize with modulo instead.
    local r = PCSX.getRegisters()
    local pc = tonumber(r.pc) % 0x100000000
    local ra = tonumber(r.GPR.n.ra) % 0x100000000
    writers_csv:row("%d,a3_1da,0x%08X,0x%08X,0x%08X,%d",
        vsync, gaza + 0x1DA, pc, ra, u8(gaza + 0x1DA))
    if pc == KILL_PC and not killed_v then
        killed_v = vsync
        PCSX.log(string.format(
            "[stale] KILL WRITE: pc=0x%08X staged idle over the Move clip at vsync %d", pc, vsync))
    end
end

local function arm_all()
    local g = u32(ACTORS + 3 * 4)
    if not in_ram(g) then
        PCSX.log("[stale] FATAL: seat-3 actor pointer invalid - wrong save state?")
        return false
    end
    gaza = g
    local rec = u32(MONREC)
    if in_ram(rec) then
        for i = 0, u8(rec + 0x4A) - 1 do
            entries[u32(rec + 0x4C + i * 4)] = i
        end
    end
    probe.arm_breakpoint(TICK_FN, "Exec", 4, "anim_tick_entry", on_tick_entry)
    probe.arm_breakpoint(gaza + 0x1DA, "Write", 1, "a3_1da", on_1da_write)
    writers:arm(gaza + 0x1D9, 1, "a3_1d9")
    writers:arm(gaza + 0x1DC, 1, "a3_1dc")
    PCSX.log(string.format(
        "[stale] armed on actor 0x%08X (+0x1DC now 0x%02X); mode=%s",
        gaza, u8(gaza + 0x1DC), STALE and "EXPERIMENT (re-arm stale bit 4)" or "CONTROL"))
    return true
end

local armed = false

local function on_vsync()
    vsync = vsync + 1
    -- Arm lazily on the first capture vsync: probe.sm calls on_arm BEFORE
    -- the save-state load, so RAM pointers are only valid from here on.
    if not armed then
        if not arm_all() then return end
        armed = true
    end
    local c = u32(CTX_PTR)
    if not (gaza and in_ram(c)) then return end

    -- Fallback node discovery: if the Exec-BP route hasn't identified the
    -- seat-3 anim node, validate the struct-shape hint (found by offline
    -- sstate scan: +0x5A slot == 3, +0x4C one of the record's entry ptrs)
    -- before trusting it.
    if not node and vsync >= 30 then
        local hint = probe.getenv_num("LEGAIA_NODE_HINT", 0x8008335C)
        if in_ram(hint) and u16(hint + 0x5A) == 3 and entries[u32(hint + 0x4C)] then
            node = hint
            PCSX.log(string.format("[stale] seat-3 anim node via validated hint = 0x%08X", node))
        end
    end

    if not poked and vsync >= POKE_V then
        if STALE then
            local f = u8(gaza + 0x1DC)
            probe.write_u8(gaza + 0x1DC, bit.bor(f, 4))
            PCSX.log(string.format(
                "[stale] re-armed +0x1DC bit 2 / mask 0x4 (0x%02X -> 0x%02X): the flag the summon staging leaves behind",
                f, u8(gaza + 0x1DC)))
        end
        probe.write_u8(c + 7, 0x14)
        PCSX.log(string.format("[stale] bounced ctx state -> 0x14 at vsync %d - hands off from here", vsync))
        poked = true
    end

    local st = u8(c + 7)
    local a1da, a1d9, a1dc = u8(gaza + 0x1DA), u8(gaza + 0x1D9), u8(gaza + 0x1DC)
    local gx, gz = i16(gaza + 0x34), i16(gaza + 0x38)
    local phase, eidx = -1, -1
    if node then
        phase = u16(node + 0x68)
        eidx = entries[u32(node + 0x4C)] or -1
    end
    local t = u32(ACTORS + u8(gaza + 0x1DD) % 8 * 4)
    local tx, tz = -1, -1
    if in_ram(t) then tx, tz = i16(t + 0x34), i16(t + 0x38) end

    if poked then
        if not engaged_v and a1d9 == 1 and a1da == 1 then engaged_v = vsync end
        if not arrived_v and st == 0x1E then arrived_v = vsync end
        if last_gx == gx and last_gz == gz then
            frozen_run = frozen_run + 1
        else
            frozen_run = 0
        end
    end
    last_gx, last_gz = gx, gz

    local row = string.format("%d,0x%02X,%d,%d,0x%02X,%d,%d,%d,%d,%d,%d,%d,%d",
        vsync, st, a1da, a1d9, a1dc, phase, eidx,
        u16(gaza + 0x176), u8(gaza + 0x21D), gx, gz, tx, tz)
    local sig = row:gsub("^%d+,", "")
    if sig ~= last_sig or vsync % 8 == 0 then
        last_sig = sig
        anim_csv:row("%s", row)
    end
end

probe.run{
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    boot_delay = 30,
    snapshot_path = probe.out_path("run.snapshot.txt"),
    -- NB: probe.sm runs on_arm BEFORE the save-state load; real arming is
    -- deferred to the first on_capture vsync (see `armed` above).
    on_arm = function() return {} end,
    on_capture = function(ctx)
        on_vsync()
        -- Early-quit once the verdict is decided (+ margin for the CSV tail).
        if STALE and killed_v and frozen_run >= 90 then ctx.request_quit = true end
        if not STALE and arrived_v and vsync > arrived_v + 120 then ctx.request_quit = true end
    end,
    on_done = function()
        local verdict
        if STALE then
            verdict = (killed_v and frozen_run >= 60 and not arrived_v)
                and "REPRODUCED: stale +0x1DC bit 2 (mask 0x4) kills the re-staged Move clip at its first natural end"
                or "INCONCLUSIVE"
        else
            verdict = (arrived_v and not killed_v)
                and "HEALTHY: mask-0x4-clear Move clip loops across natural ends and arrives"
                or "INCONCLUSIVE"
        end
        local lines = {
            string.format("=== gaza2 stale-flag repro (%s): %s ===",
                STALE and "EXPERIMENT" or "CONTROL", verdict),
            string.format("vsyncs=%d poke@=%d node=%s", vsync, POKE_V,
                node and string.format("0x%08X", node) or "NOT FOUND"),
            string.format("Move engaged (pair 1/1) at: %s", tostring(engaged_v)),
            string.format("kill write (pc=0x%08X sb zero,+0x1DA) at: %s",
                KILL_PC, tostring(killed_v)),
            string.format("clip lifetime engage->kill: %s vsyncs",
                (engaged_v and killed_v) and tostring(killed_v - engaged_v) or "n/a"),
            string.format("state 0x1E (arrived) at: %s", tostring(arrived_v)),
            string.format("final: ctx7=0x%02X pair=%d/%d +0x1DC=0x%02X pos=(%d,%d) frozen_run=%d",
                u8(u32(CTX_PTR) + 7), u8(gaza + 0x1DA), u8(gaza + 0x1D9),
                u8(gaza + 0x1DC), i16(gaza + 0x34), i16(gaza + 0x38), frozen_run),
            string.format("writer-watch hits: %d (see writers.csv)", writers:total()),
        }
        probe.write_snapshot(probe.out_path("summary.txt"), table.concat(lines, "\n"))
        for _, l in ipairs(lines) do PCSX.log("[stale] " .. l) end
    end,
}
