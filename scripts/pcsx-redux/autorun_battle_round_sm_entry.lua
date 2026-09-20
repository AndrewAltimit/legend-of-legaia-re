-- autorun_battle_round_sm_entry.lua
--
-- Does an ordinary, non-dome battle enter `FUN_801D0748`?
--
-- `FUN_801D0748` (PROT 0898, base 0x801CE818) is labelled the Muscle Dome
-- match state machine, yet its arms are general battle features: the Attack
-- confirm at `0x801D15C8` (`sb 3, 0x1DE(v1)` in the delay slot of
-- `jal 0x801DA34C`), the arts-input entry at `0x801D1734` (same callee), and
-- the auto-command write-back at `0x801D22BC` (`jal 0x801DA59C`). Its own
-- prologue loads the battle context pointer `*0x8007BD24` and switches on the
-- flow byte `ctx[+0x06]` against `0x1E / 0x32 / 0x6E / 0xFE` - the same flow
-- byte the battle command-menu probes walk. No capture had caught it running.
--
-- This probe taps the entry plus the three arms and buckets the hits by the
-- live flow byte, while a pad ladder walks the command ring and confirms
-- Attack. A single entry in a field/tutorial battle settles the label.
--
-- Taps assert the word at each address, so a mis-based or paged-out tap
-- reports MISMATCH rather than reading as a zero census:
--   0x801D0748 = 0x3C028008 (lui v0,0x8008)
--   0x801D15C8 = 0x0C0768D3 (jal 0x801DA34C)
--   0x801D1734 = 0x0C0768D3 (jal 0x801DA34C)
--   0x801D22BC = 0x0C076967 (jal 0x801DA59C)
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh \
--       --iso <a PPF-free copy of the disc> \
--       --scenario v0_1_battle_command_menu \
--       --lua scripts/pcsx-redux/autorun_battle_round_sm_entry.lua \
--       --frames 700
--
-- Env vars:
--   LEGAIA_SSTATE   save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES   capture vsyncs (default 700)
--   LEGAIA_NOPAD    1 = observe only, do not drive the pad
--
-- Outputs: battle_round_sm_entry.csv (per-entry rows), .log (summary)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 700)
local NOPAD  = probe.getenv_num("LEGAIA_NOPAD", 0)
local ROWS   = probe.getenv_num("LEGAIA_ROWS", 3000)

local OUT_CSV = probe.out_path("battle_round_sm_entry.csv")
local OUT_LOG = probe.out_path("battle_round_sm_entry.log")

local CTX_PTR   = 0x8007BD24
local GAME_MODE = 0x8007B83C
local SCENE     = 0x8007050C

local TAPS = {
    { addr = 0x801D0748, want = 0x3C028008, kind = "entry",
      name = "FUN_801D0748 entry" },
    { addr = 0x801D15C8, want = 0x0C0768D3, kind = "attack",
      name = "Attack confirm (sb 3 -> +0x1DE)" },
    { addr = 0x801D1734, want = 0x0C0768D3, kind = "arts_in",
      name = "arts-input entry (phase 0x50)" },
    { addr = 0x801D22BC, want = 0x0C076967, kind = "autocmd",
      name = "auto-command write-back" },
}

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[roundsm] " .. s)
end

local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end
local function hex8(v) return string.upper(bit.tohex(n32(v))) end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE + i)
        if b == nil or b < 0x20 or b >= 0x7F then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function flow_byte()
    local c = probe.read_u32(CTX_PTR)
    if c == nil or c < 0x80000000 or c >= 0x80200000 then return nil end
    return probe.read_u8(c + 6)
end

local csv
local g_elapsed = 0
local counts = {}       -- kind -> hits
local ra_seen = {}      -- kind -> { ra -> n }
local flow_at_entry = {}
local flow_trace = {}
local last_flow = nil
local rows = 0

-- Pad ladder: nudge the command ring and confirm, then let the round run.
-- LEFT walks the Begin/Run prompt (0x1E) into the command ring (0x28); CROSS
-- confirms whatever the ring is on. Repeated every `PERIOD` vsyncs so a state
-- parked anywhere in the round still advances.
local PERIOD = 90
local LADDER = { probe.BTN.LEFT, probe.BTN.CROSS, probe.BTN.CROSS,
                 probe.BTN.CROSS, probe.BTN.CROSS, probe.BTN.CROSS }
local press_at, press_btn, ladder_i = nil, nil, 1

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        csv = probe.csv_open(OUT_CSV, "seq,vsync,tap,flow,ra,mode,scene")
        probe.env.write_manifest("autorun_battle_round_sm_entry.lua", {
            sstate = SSTATE, frames = FRAMES, nopad = NOPAD,
        })
        local descs = {}
        for _, t in ipairs(TAPS) do
            local d = { addr = t.addr, hits_ref = { n = 0 }, name = t.name }
            ra_seen[t.kind] = {}
            probe.arm_breakpoint(t.addr, "Exec", 4, t.kind, function()
                d.hits_ref.n = d.hits_ref.n + 1
                counts[t.kind] = (counts[t.kind] or 0) + 1
                local r = PCSX.getRegisters()
                local ra = hex8(r.GPR.n.ra)
                ra_seen[t.kind][ra] = (ra_seen[t.kind][ra] or 0) + 1
                local f = flow_byte()
                if t.kind == "entry" and f ~= nil then
                    flow_at_entry[f] = (flow_at_entry[f] or 0) + 1
                end
                rows = rows + 1
                if rows <= ROWS then
                    csv:row("%d,%d,%s,%s,0x%s,%d,%s", rows, g_elapsed, t.kind,
                        f and string.format("0x%02X", f) or "nil", ra,
                        probe.read_u8(GAME_MODE) or 0, scene_name())
                end
            end)
            descs[#descs + 1] = d
        end
        return descs
    end,

    on_capture = function(ctx, el)
        g_elapsed = el
        if el == 2 then
            local bad = 0
            for _, t in ipairs(TAPS) do
                local got = n32(probe.read_u32(t.addr) or 0)
                if got ~= n32(t.want) then
                    bad = bad + 1
                    logf("TAP MISMATCH [0x%08X] = 0x%s want 0x%s (%s)",
                        t.addr, hex8(got), hex8(t.want), t.name)
                end
            end
            logf("armed %d taps, %d mismatched; mode=%d scene=%s flow=%s",
                #TAPS, bad, probe.read_u8(GAME_MODE) or 0, scene_name(),
                tostring(flow_byte()))
        end

        local f = flow_byte()
        if f ~= last_flow then
            flow_trace[#flow_trace + 1] =
                string.format("%d:0x%02X", el, f or 0xFF)
            last_flow = f
        end

        if press_at ~= nil and el >= press_at + 4 then
            probe.pad_release(press_btn)
            press_at = nil
        end
        if NOPAD == 0 and press_at == nil and el >= 40 and (el % PERIOD) == 0
           and ladder_i <= #LADDER then
            press_btn = LADDER[ladder_i]
            probe.pad_force(press_btn)
            press_at = el
            ladder_i = ladder_i + 1
            logf("f=%d press btn %d (ladder %d) flow=%s",
                el, press_btn, ladder_i - 1,
                f and string.format("0x%02X", f) or "nil")
        end

        if (el % 120) == 0 then
            logf("vsync %d entry=%d attack=%d arts=%d autocmd=%d flow=%s",
                el, counts.entry or 0, counts.attack or 0,
                counts.arts_in or 0, counts.autocmd or 0,
                f and string.format("0x%02X", f) or "nil")
        end
    end,

    on_summary = function()
        logf("--- FUN_801D0748 entry census over %d vsyncs ---", g_elapsed)
        logf("scene=%s mode=%d", scene_name(), probe.read_u8(GAME_MODE) or 0)
        logf("flow trace: %s", table.concat(flow_trace, " "))
        for _, t in ipairs(TAPS) do
            logf("%-8s 0x%08X entries: %d  (%s)",
                t.kind, t.addr, counts[t.kind] or 0, t.name)
            local ras = {}
            for ra, n in pairs(ra_seen[t.kind] or {}) do
                ras[#ras + 1] = { ra, n }
            end
            table.sort(ras, function(a, b) return a[2] > b[2] end)
            for i = 1, math.min(#ras, 6) do
                logf("    ra 0x%s : %d", ras[i][1], ras[i][2])
            end
        end
        local fs = {}
        for f, n in pairs(flow_at_entry) do fs[#fs + 1] = { f, n } end
        table.sort(fs, function(a, b) return a[2] > b[2] end)
        for i = 1, math.min(#fs, 16) do
            logf("  entry with flow 0x%02X : %d", fs[i][1], fs[i][2])
        end
        local fh = io.open(OUT_LOG, "w")
        if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
        if csv then csv:close() end
    end,
})
