-- autorun_fishing_lure_drift.lua
--
-- Does the cast lure's sideways drift really take its SIGN from the low bit
-- of the persistent counter at `0x80084460`?
--
-- The bite tick `FUN_801D26CC` calls the walk-grid probe `FUN_801D7030`
-- (`jal` at `0x801D2E10`). On a hit it pushes the lure's 24.8 `x`
-- accumulator `0x801D9174` by `frame_delta << 11`, and the two arms of that
-- push are separate code:
--
--   0x801D2E28  andi $v0,$v0,1     <- (*0x80084460) & 1
--   0x801D2E2C  beqz $v0,0x801D2E48
--   0x801D2E34  ADD arm  (lbu 0x7f($s2); sll 0xb; addu)
--   0x801D2E48  SUB arm  (lbu 0x7f($s2); sll 0xb; subu)
--   0x801D2E58  sw $v1,-0x6e8c($s1)   <- the store into 0x801D9174
--
-- so an exec tap on each arm, carrying the counter it was reached with, is a
-- direct test: every ADD hit must carry an ODD counter and every SUB hit an
-- EVEN one.
--
-- The counter's only displacement-form access disc-wide is that `lw`; its
-- writer reaches it through the SC-block base instead (`t0 = 0x80084140`,
-- `lw/addiu/sw 0x320($t0)` at `0x801D2954`..`0x801D296C`, in the same bite
-- tick), which is why a scan for the `0x4460` displacement finds only the
-- read. The write is one-per-hook, so the sign alternates between casts -
-- and `LEGAIA_FLIP_PERIOD` pokes it as well, so a short run still sees both.
--
-- The drift only runs when the walk-grid probe returns non-zero, which the
-- pond the scenario parks on never does. `LEGAIA_FORCE_DRIFT` therefore
-- forces `$v0 = 1` at the `beqz` (0x801D2E18) when the grid said no, so the
-- sign arms get exercised. That forcing changes WHETHER the drift runs, not
-- which way it goes - the summary reports natural and forced verdicts apart.
--
-- The cast itself is `FUN_801CF3BC` case `0x14`; its spawn writes the tracked
-- halfword triple at `0x801D918C` (x, y-0x80, z) and the `<< 8` accumulators
-- at `0x801D9174` / `0x801D9178` / `0x801D917C`. The tap at `0x801CFC50`
-- (`addiu $a1,$zero,0xc8`, the polar radius the spawn subtracts) fires once
-- per cast lock.
--
-- Pad driving: the mode word `0x801D926C` picks which button to pulse, so the
-- run walks rod-select -> cast wind-up -> power oscillator -> lock without a
-- fixed frame script (docs/subsystems/minigame-fishing.md state table).
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --iso <a PPF-free copy of the disc> \
--       --scenario minigame_fishing_pcsx \
--       --lua scripts/pcsx-redux/autorun_fishing_lure_drift.lua \
--       --frames 5400
--
-- Env vars:
--   LEGAIA_SSTATE        save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES        capture vsyncs (default 5400)
--   LEGAIA_FLIP_PERIOD   vsyncs between counter pokes (default 900, 0 = off)
--   LEGAIA_TRACE_EVERY   vsyncs between per-frame state rows (default 1)
--   LEGAIA_POWER_LOCK    cast-meter value to lock at (default 0xE00)
--   LEGAIA_QUIET_AFTER_CAST  vsyncs of no pad after a lock (default 240)
--   LEGAIA_FORCE_DRIFT   1 = force the grid verdict true (default 1)
--
-- Outputs: fishing_lure_drift.csv (per-vsync state), .hits.csv (arm hits),
--          .log (summary).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")

local SSTATE       = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES       = probe.getenv_num("LEGAIA_FRAMES", 5400)
local FLIP_PERIOD  = probe.getenv_num("LEGAIA_FLIP_PERIOD", 900)
local TRACE_EVERY  = probe.getenv_num("LEGAIA_TRACE_EVERY", 1)
local POWER_LOCK   = probe.getenv_num("LEGAIA_POWER_LOCK", 0xE00)
local QUIET_AFTER_CAST = probe.getenv_num("LEGAIA_QUIET_AFTER_CAST", 240)
local FORCE_DRIFT   = probe.getenv_num("LEGAIA_FORCE_DRIFT", 1)

local OUT_CSV  = probe.out_path("fishing_lure_drift.csv")
local OUT_HITS = probe.out_path("fishing_lure_drift.hits.csv")
local OUT_LOG  = probe.out_path("fishing_lure_drift.log")

-- Fishing overlay globals (docs/subsystems/minigame-fishing.md).
local SM_STATE   = 0x801D926C  -- DAT_801d926c mode word
local LURE_X_FIX = 0x801D9174  -- 24.8 x accumulator (the drift target)
local LURE_Y_FIX = 0x801D9178
local LURE_Z_FIX = 0x801D917C
local TRACK_B    = 0x801D918C  -- i16 x at +0, y-0x80 at +2, z at +4
local CAST_POWER = 0x801D9274
local COUNTER    = 0x80084460  -- the word whose low bit signs the drift
local FRAME_STEP = 0x1F800393  -- scratchpad frame delta
-- The pad word the fishing SM's own arms read (`lw $v0,-0x478c($a1)` with
-- `lui $a1,0x8008` at 0x801CF994 / 0x801CFB9C), NOT the `_DAT_8007B850`
-- copy the subsystem doc names: both the idle arm (state 0xC) and the
-- cast-power arm (state 0x14) advance on `andi 0xc0` = Cross | Square.
local PAD_MASK   = 0x8007B874
local PAD_CAST   = 0xC0

-- Exec taps, each with the word that must be at the address (a fingerprint
-- check, so a mis-based address shows up as MISMATCH instead of silence).
local TAPS = {
    { addr = 0x801D2E10, want = 0x0C075C0C, kind = "probe_call",
      name = "jal FUN_801D7030 (walk-grid probe)" },
    { addr = 0x801D2E18, want = 0x10400010, kind = "probe_result",
      name = "beqz $v0 (the walk-grid probe's verdict)" },
    { addr = 0x801D2E34, want = 0x9242007F, kind = "drift_add",
      name = "ADD arm (counter bit set)" },
    { addr = 0x801D2E48, want = 0x9242007F, kind = "drift_sub",
      name = "SUB arm (counter bit clear)" },
    { addr = 0x801D2E58, want = 0xAE239174, kind = "drift_store",
      name = "sw into 0x801D9174" },
    { addr = 0x801CFC50, want = 0x240500C8, kind = "cast_lock",
      name = "cast spawn polar radius 200" },
}

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[lure] " .. s)
end

local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end
local function hex8(v) return string.upper(bit.tohex(n32(v))) end
local function s32(v)
    v = n32(v)
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end
local function s16(v)
    v = bit.band(tonumber(v) or 0, 0xFFFF)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local csv, hits
local g_elapsed = 0
local counts = {}
local add_odd, add_even, sub_odd, sub_even = 0, 0, 0, 0
local casts = 0
local hit_rows = 0
local flips = 0
local states_seen = {}
local last_state = nil
local quiet_until = nil
local natural_hits, forced_hits = 0, 0

local function counter() return n32(probe.read_u32(COUNTER) or 0) end
-- The frame delta is a scratchpad BYTE; `probe.read_u32` does not map the
-- scratchpad at all, so this goes through the byte-granular reader.
local function step() return mem.read_scratch_u8(FRAME_STEP) or 0 end

-- The scenario parks BEFORE the fishing overlay is paged in, so the taps'
-- fingerprints only mean anything once the image is resident. Re-check them
-- every vsync until they all match, and say which vsync that was.
local resident_at = nil
local function fingerprints_match()
    for _, t in ipairs(TAPS) do
        if n32(probe.read_u32(t.addr) or 0) ~= n32(t.want) then
            return false
        end
    end
    return true
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        csv = probe.csv_open(OUT_CSV,
            "vsync,state,power,counter,step,x_fixed,y_fixed,z_fixed,"
            .. "track_x,track_y,track_z,pad_mask")
        hits = probe.csv_open(OUT_HITS,
            "seq,vsync,arm,counter,counter_bit,step,x_fixed_before,"
            .. "x_fixed_after,expect_delta,state")
        probe.env.write_manifest("autorun_fishing_lure_drift.lua", {
            sstate = SSTATE, frames = FRAMES,
            flip_period = FLIP_PERIOD, trace_every = TRACE_EVERY,
        })
        local descs = {}
        for _, t in ipairs(TAPS) do
            local d = { addr = t.addr, hits_ref = { n = 0 }, name = t.name }
            probe.arm_breakpoint(t.addr, "Exec", 4, t.kind, function()
                d.hits_ref.n = d.hits_ref.n + 1
                counts[t.kind] = (counts[t.kind] or 0) + 1
                if t.kind == "cast_lock" then
                    casts = casts + 1
                    return
                end
                if t.kind == "probe_result" then
                    -- `beqz $v0` decides whether the drift runs at all. Count
                    -- how often the pond's own grid says yes, and - under
                    -- FORCE_DRIFT - force a yes so the SIGN arms get
                    -- exercised even where the venue never sets the bit.
                    local r = PCSX.getRegisters()
                    if n32(r.GPR.n.v0) ~= 0 then
                        natural_hits = natural_hits + 1
                    elseif FORCE_DRIFT ~= 0 then
                        r.GPR.n.v0 = 1
                        forced_hits = forced_hits + 1
                    end
                    return
                end
                if t.kind ~= "drift_add" and t.kind ~= "drift_sub" then
                    return
                end
                local c = counter()
                local bit0 = bit.band(c, 1)
                if t.kind == "drift_add" then
                    if bit0 == 1 then add_odd = add_odd + 1
                    else add_even = add_even + 1 end
                else
                    if bit0 == 1 then sub_odd = sub_odd + 1
                    else sub_even = sub_even + 1 end
                end
                local st = step()
                local before = s32(probe.read_u32(LURE_X_FIX) or 0)
                local delta = st * 2048
                if t.kind == "drift_sub" then delta = -delta end
                hit_rows = hit_rows + 1
                if hit_rows <= 4000 then
                    hits:row("%d,%d,%s,%d,%d,%d,%d,%d,%d,%d",
                        hit_rows, g_elapsed, t.kind, c, bit0, st,
                        before, before + delta, delta,
                        n32(probe.read_u32(SM_STATE) or 0))
                end
            end)
            descs[#descs + 1] = d
        end
        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if elapsed == 2 then
            logf("start state=0x%X counter=%d",
                 n32(probe.read_u32(SM_STATE) or 0), counter())
        end
        if not resident_at and fingerprints_match() then
            resident_at = elapsed
            logf("fishing overlay resident at vsync %d; all %d tap "
                 .. "fingerprints match", elapsed, #TAPS)
        elseif not resident_at and (elapsed % 300) == 0 then
            for _, t in ipairs(TAPS) do
                local got = n32(probe.read_u32(t.addr) or 0)
                logf("vsync %d not-yet-resident [0x%08X] = 0x%s (want 0x%s, %s)",
                     elapsed, t.addr, hex8(got), hex8(t.want), t.name)
            end
        end

        if resident_at and (elapsed % 600) == 0 then
            logf("vsync %d taps: probe_call=%d add=%d sub=%d store=%d "
                 .. "cast=%d natural=%d forced=%d",
                 elapsed, counts.probe_call or 0, counts.drift_add or 0,
                 counts.drift_sub or 0, counts.drift_store or 0,
                 counts.cast_lock or 0, natural_hits, forced_hits)
        end

        local st = n32(probe.read_u32(SM_STATE) or 0)
        if st ~= last_state then
            states_seen[#states_seen + 1] =
                string.format("%d:0x%X", elapsed, st)
            last_state = st
        end

        if TRACE_EVERY > 0 and (elapsed % TRACE_EVERY) == 0 then
            csv:row("%d,0x%X,%d,%d,%d,%d,%d,%d,%d,%d,%d,0x%X",
                elapsed, st,
                s32(probe.read_u32(CAST_POWER) or 0),
                counter(), step(),
                s32(probe.read_u32(LURE_X_FIX) or 0),
                s32(probe.read_u32(LURE_Y_FIX) or 0),
                s32(probe.read_u32(LURE_Z_FIX) or 0),
                s16(probe.read_u16(TRACK_B) or 0),
                s16(probe.read_u16(TRACK_B + 2) or 0),
                s16(probe.read_u16(TRACK_B + 4) or 0),
                n32(probe.read_u32(PAD_MASK) or 0))
        end

        -- Flip the counter's low bit periodically: nothing in the fishing
        -- overlay writes 0x80084460 (one `lw` disc-wide, at 0x801D2E20), so a
        -- session would otherwise exercise only one of the two arms.
        if FLIP_PERIOD > 0 and elapsed > 0 and (elapsed % FLIP_PERIOD) == 0 then
            local c = counter()
            probe.write_u32(COUNTER, bit.bxor(c, 1))
            flips = flips + 1
            logf("vsync %d: counter %d -> %d (flip #%d)",
                 elapsed, c, bit.bxor(c, 1), flips)
        end

        -- Pad driving. Cross is the confirm AND the cast on both arms that
        -- gate on `andi 0xc0`, so one button walks the whole chain. Two
        -- refinements the first runs needed:
        --   * state 0x14 is the power oscillator; pressing on its first frame
        --     locks the cast near the 0x20 floor and the lure lands short, so
        --     wait for the meter to climb past POWER_LOCK first.
        --   * once the cast is locked, go quiet for QUIET_AFTER_CAST vsyncs -
        --     the lure's own travel / bite tick is what the drift sites live
        --     in, and another press restarts the session instead.
        if quiet_until and elapsed < quiet_until then
            probe.pad_release(probe.BTN.CROSS)
        elseif st == 0x14 then
            local pw = s32(probe.read_u32(CAST_POWER) or 0)
            if pw >= POWER_LOCK then
                probe.pad_force(probe.BTN.CROSS)
                quiet_until = elapsed + QUIET_AFTER_CAST
            else
                probe.pad_release(probe.BTN.CROSS)
            end
        else
            local phase = elapsed % 24
            if phase == 0 then
                probe.pad_force(probe.BTN.CROSS)
            elseif phase == 4 then
                probe.pad_release(probe.BTN.CROSS)
            end
        end
    end,

    on_summary = function()
        probe.pad_release(probe.BTN.CIRCLE)
        probe.pad_release(probe.BTN.CROSS)
        local ks = {}
        for k, n in pairs(counts) do ks[#ks + 1] = string.format("%s=%d", k, n) end
        table.sort(ks)
        logf("tap hits: %s", table.concat(ks, " "))
        logf("ADD arm: odd counter %d / even counter %d", add_odd, add_even)
        logf("SUB arm: odd counter %d / even counter %d", sub_odd, sub_even)
        logf("cast locks=%d counter flips=%d overlay resident at vsync %s",
             casts, flips, tostring(resident_at))
        logf("walk-grid verdicts: natural hits %d / forced %d of %d calls "
             .. "(force_drift=%d)",
             natural_hits, forced_hits, counts.probe_result or 0, FORCE_DRIFT)
        logf("sign rule holds: %s",
             tostring(add_even == 0 and sub_odd == 0
                      and (add_odd + sub_even) > 0))
        logf("state transitions: %s", table.concat(states_seen, " "))
        local fh = io.open(OUT_LOG, "w")
        if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
        if csv then csv:close() end
        if hits then hits:close() end
    end,
})
