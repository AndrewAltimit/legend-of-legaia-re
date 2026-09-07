-- autorun_w4d_cort_flow_writer.lua
--
-- Who moves the battle command-flow byte `ctx[+0x06]` off `0x0C`?
--
-- The evolved-Cort fight is the one battle whose intro arm parks the flow byte
-- on a value the `beq` ladder at `0x801D0C84` has no arm for: `FUN_801D9D3C`'s
-- caller compares monster-slot-0's id (`0x8007BD0C`) against `0xB5`, skips the
-- banner composer and writes `ctx[+0x06] = 0x0C` instead of `0x0B`
-- (docs/subsystems/battle.md, "One monster opens its fight with no banner").
-- The fight still opens in play, so some writer outside that ladder moves the
-- byte on. This probe finds it and says what it was waiting for.
--
-- Instrumentation:
--   * a width-1 Write watch on `ctx+0x06` (ctx = `*_DAT_8007BD24`), armed as
--     soon as the pointer is live, logging the faulting `pc` / `ra` and the
--     value ABOUT TO BE STORED - decoded out of the store instruction's source
--     register, because the debug hook runs before the store and memory still
--     holds the old byte.
--   * a per-vsync poll of flow `ctx[+0x06]`, action state `ctx[+0x07]`, the
--     intro timer `ctx[+0x6D6]`, the ambush flag `ctx[+0x290]`, the round
--     counter `ctx[+0x28A]`, game mode and monster-slot-0 id.
--
-- Pad plan (the "what is it waiting on" half). The run is deliberately
-- INPUT-FREE until the park is reproduced: once the flow byte has held `0x0C`
-- for LEGAIA_IDLE vsyncs, the probe sweeps one button at a time for
-- LEGAIA_SWEEP vsyncs each and records which button (if any) was held when the
-- byte moved. A write during the idle phase is itself the answer - the gate is
-- a timer / asset barrier, not an input.
--
-- Env vars:
--   LEGAIA_SSTATE   save-state path (or run_probe.sh --scenario cort_evolved_pre_battle)
--   LEGAIA_FRAMES   capture vsyncs (default 3000)
--   LEGAIA_IDLE     pad-free vsyncs after the park before the sweep (default 240;
--                   a negative value never sweeps - pure observation)
--   LEGAIA_SWEEP    vsyncs per swept button (default 60)
--   LEGAIA_STAGE    1 = also breakpoint the PROT 0968 stage module's entry and
--                   its seven phase arms (default 1; 0 = flow watch only)
--   LEGAIA_OUT_DIR  output directory (run_probe.sh sets this)
--
-- Outputs: w4d_cort_flow.csv (per-write + per-transition rows),
--          w4d_cort_flow.log, w4d_cort_flow.detail.txt (call contexts).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE  = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES  = probe.getenv_num("LEGAIA_FRAMES", 3000)
local IDLE    = probe.getenv_num("LEGAIA_IDLE", 240)
local SWEEP   = probe.getenv_num("LEGAIA_SWEEP", 60)
local STAGE   = probe.getenv_num("LEGAIA_STAGE", 1)

local OUT_LOG    = probe.out_path("w4d_cort_flow.log")
local OUT_CSV    = probe.out_path("w4d_cort_flow.csv")
local OUT_DETAIL = probe.out_path("w4d_cort_flow.detail.txt")

-- SCUS globals (docs/subsystems/battle.md, battle-action.md).
local CTX_PTR    = 0x8007BD24   -- _DAT_8007BD24 -> live battle ctx
local GAME_MODE  = 0x8007B83C
local FORMATION  = 0x8007BD0C   -- monster slot ids; [0] == 0xB5 is evolved Cort
local SCENE_NAME = 0x8007050C
local LOADER_B   = 0x8007BC4C   -- loader-B tracker (0x49 = the stage overlay)
local SLOT_B     = 0x801F69D8   -- slot-B overlay base

-- The evolved-Cort stage module (PROT 0968) at slot B. Its head is a 7-word
-- jump table indexed by the phase byte ctx[+0x289]; the last arm is the one
-- that hands the flow byte back. Addresses read off the image with
-- scripts/ghidra-analysis/disasm-overlay-fn.py --base 0x801F69D8.
local STAGE_ENTRY = 0x801F69F4
local STAGE_ARMS  = {
    0x801F6A74, 0x801F6B98, 0x801F6CD4, 0x801F6E0C,
    0x801F6F3C, 0x801F7004, 0x801F70D8,
}
local STAGE_HANDBACK = 0x801F713C   -- sb v0,6(v1) with v0 = 0x0B

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[w4d_cort] " .. s)
end

local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end

-- The debug hook runs BEFORE the store, so re-reading the watched byte yields
-- the value being overwritten. Decode the store at PC and read its source
-- register to get the value that is about to land.
local function stored_value()
    local r    = PCSX.getRegisters()
    local pc   = n32(r.pc)
    local insn = probe.read_u32(pc)
    if insn == nil then return nil, pc end
    local op = bit.rshift(n32(insn), 26)
    local rt = bit.band(bit.rshift(n32(insn), 16), 0x1F)
    local v  = n32(r.GPR.r[rt])
    if op == 0x28 then return bit.band(v, 0xFF), pc
    elseif op == 0x29 then return bit.band(v, 0xFFFF), pc
    elseif op == 0x2B then return v, pc end
    return nil, pc
end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME + i)
        if b == nil or b < 0x20 or b >= 0x7F then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function ctx_base()
    local p = probe.read_u32(CTX_PTR)
    if p == nil or p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end

-- Buttons swept after the park, in the order a player would try them.
local SWEEP_BTNS = {
    { name = "CROSS",    btn = probe.BTN.CROSS },
    { name = "START",    btn = probe.BTN.START },
    { name = "CIRCLE",   btn = probe.BTN.CIRCLE },
    { name = "UP",       btn = probe.BTN.UP },
    { name = "LEFT",     btn = probe.BTN.LEFT },
    { name = "RIGHT",    btn = probe.BTN.RIGHT },
    { name = "DOWN",     btn = probe.BTN.DOWN },
    { name = "TRIANGLE", btn = probe.BTN.TRIANGLE },
    { name = "SQUARE",   btn = probe.BTN.SQUARE },
    { name = "SELECT",   btn = probe.BTN.SELECT },
}

local held_name, held_btn = "none", nil
local function hold(entry)
    if held_btn then probe.pad_release(held_btn) end
    held_btn  = entry and entry.btn or nil
    held_name = entry and entry.name or "none"
    if held_btn then probe.pad_force(held_btn) end
end

local csv = nil
local g_elapsed = 0             -- live frame counter; breakpoint callbacks
                                -- outlive the on_capture closure that armed
                                -- them, so they must NOT capture `elapsed`.
local armed_watch = false
local writes = 0
local park_since = nil          -- first vsync the byte read 0x0C
local sweep_idx, sweep_start = 0, nil
local last_flow, last_mode, last_state = nil, nil, nil
local last_trk, last_sig = nil, nil

-- 16-word FNV-1a over the head of slot B: names WHICH module is resident when
-- a flow write lands. Slot A and slot B are both VA-aliased across overlays, so
-- an address alone says nothing about which module owns it.
local function slot_b_sig()
    local h = 2166136261
    for i = 0, 15 do
        local w = probe.read_u32(SLOT_B + i * 4) or 0
        for b = 0, 3 do
            h = bit.bxor(h, bit.band(bit.rshift(w, b * 8), 0xFF))
            h = bit.band(h * 16777619, 0xFFFFFFFF)
        end
    end
    return bit.band(h, 0xFFFFFFFF)
end

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        csv = probe.csv_open(OUT_CSV,
            "tick,event,flow,newflow,pc,ra,mode,timer,held,note")
        ctx.write_sites = {}
        ctx.stage_hits  = {}
        if STAGE ~= 0 then
            probe.arm_breakpoint(STAGE_ENTRY, "Exec", 4, "stage_entry", function()
                ctx.stage_hits["entry"] = (ctx.stage_hits["entry"] or 0) + 1
            end)
            for i, a in ipairs(STAGE_ARMS) do
                local key = string.format("arm%d 0x%08X", i - 1, a)
                probe.arm_breakpoint(a, "Exec", 4,
                    string.format("stage_arm%d", i - 1), function()
                    ctx.stage_hits[key] = (ctx.stage_hits[key] or 0) + 1
                end)
            end
            probe.arm_breakpoint(STAGE_HANDBACK, "Exec", 4, "stage_handback",
                function()
                ctx.stage_hits["handback 0x801F713C"] =
                    (ctx.stage_hits["handback 0x801F713C"] or 0) + 1
                logf("STAGE HANDBACK at vsync %d: 0968 writes ctx+6", g_elapsed)
            end)
        end
        probe.write_manifest("autorun_w4d_cort_flow_writer.lua", {
            sstate = SSTATE, frames = FRAMES, idle = IDLE, sweep = SWEEP,
            core = probe.getenv("LEGAIA_CORE", "?"),
        })
        return {}
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        local base = ctx_base()
        local mode = probe.read_u8(GAME_MODE) or 0

        -- Arm the ctx-relative watch the moment the pointer is live. It is a
        -- byte store, so the watch must be width 1: a wider watch at +0x06
        -- would still match, but a narrower address window would not.
        if base and not armed_watch then
            armed_watch = true
            logf("ctx base = 0x%08X at vsync %d (mode 0x%02X scene=%s)",
                base, elapsed, mode, scene_name())
            probe.arm_breakpoint(base + 6, "Write", 1, "flow_write", function()
                writes = writes + 1
                local r  = PCSX.getRegisters()
                local ra = n32(r.GPR.n.ra)
                local v, pc = stored_value()
                local prev = probe.read_u8(base + 6) or 0
                local key = string.format("pc=0x%08X ra=0x%08X trkB=0x%02X",
                    pc, ra, probe.read_u8(LOADER_B) or 0)
                ctx.write_sites[key] = (ctx.write_sites[key] or 0) + 1
                csv:row("%d,write,0x%02X,0x%02X,0x%08X,0x%08X,0x%02X,%d,%s,%s",
                    g_elapsed, prev, v or 0xFF, pc, ra,
                    probe.read_u8(GAME_MODE) or 0,
                    probe.read_u16(base + 0x6D6) or 0, held_name, "")
                if prev ~= (v or prev) then
                    logf("WRITE vsync %d  ctx+6: 0x%02X -> 0x%02X  pc=0x%08X ra=0x%08X held=%s timer=%d",
                        g_elapsed, prev, v or 0, pc, ra, held_name,
                        probe.read_u16(base + 0x6D6) or 0)
                end
                if writes <= 24 then
                    probe.append_call_context(OUT_DETAIL,
                        probe.capture_call_context(string.format(
                            "ctx+6 write #%d prev=0x%02X new=0x%02X held=%s vsync=%d",
                            writes, prev, v or 0, held_name, g_elapsed)))
                end
            end)
        end

        local trk = probe.read_u8(LOADER_B) or 0
        local sig = slot_b_sig()
        if trk ~= last_trk or sig ~= last_sig then
            logf("SLOT-B vsync %d: loader-B tracker 0x%02X, head sig 0x%08X",
                elapsed, trk, sig)
            csv:row("%d,slotb,,,,,0x%02X,,%s,trk=0x%02X sig=0x%08X",
                elapsed, mode, held_name, trk, sig)
            last_trk, last_sig = trk, sig
        end

        if mode ~= last_mode then
            logf("MODE 0x%02X -> 0x%02X at vsync %d (scene=%s formation0=0x%02X)",
                last_mode or 0, mode, elapsed, scene_name(),
                probe.read_u8(FORMATION) or 0)
            csv:row("%d,mode,,,,,0x%02X,,%s,scene=%s", elapsed, mode, held_name,
                scene_name())
            last_mode = mode
        end
        if base == nil then return end

        local flow  = probe.read_u8(base + 6) or 0
        local state = probe.read_u8(base + 7) or 0
        local timer = probe.read_u16(base + 0x6D6) or 0
        if flow ~= last_flow or state ~= last_state then
            logf("vsync %4d flow 0x%02X->0x%02X state 0x%02X->0x%02X timer=%d phase=%d amb=%d round=%d held=%s",
                elapsed, last_flow or 0, flow, last_state or 0, state, timer,
                probe.read_u8(base + 0x289) or 0,
                probe.read_u8(base + 0x290) or 0,
                probe.read_u16(base + 0x28A) or 0, held_name)
            csv:row("%d,flow,0x%02X,0x%02X,,,0x%02X,%d,%s,state=0x%02X",
                elapsed, last_flow or 0, flow, mode, timer, held_name, state)
            last_flow, last_state = flow, state
        end

        -- Park detection + button sweep.
        if flow == 0x0C then
            if park_since == nil then
                park_since = elapsed
                logf("PARK: flow 0x0C first seen at vsync %d (timer=%d)", elapsed, timer)
            end
            if IDLE >= 0 and sweep_start == nil and (elapsed - park_since) >= IDLE then
                sweep_idx, sweep_start = 1, elapsed
                hold(SWEEP_BTNS[1])
                logf("idle phase over (%d vsyncs parked, no input): sweeping %s",
                    IDLE, SWEEP_BTNS[1].name)
            elseif sweep_start ~= nil and (elapsed - sweep_start) >= SWEEP then
                sweep_idx = sweep_idx + 1
                sweep_start = elapsed
                if sweep_idx <= #SWEEP_BTNS then
                    hold(SWEEP_BTNS[sweep_idx])
                    logf("sweep %d: hold %s (vsync %d)", sweep_idx,
                        SWEEP_BTNS[sweep_idx].name, elapsed)
                else
                    sweep_idx, sweep_start = 1, elapsed
                    hold(SWEEP_BTNS[1])
                    logf("sweep wrapped at vsync %d", elapsed)
                end
            end
        elseif park_since ~= nil and flow ~= 0x0C then
            logf("LEFT PARK at vsync %d: flow now 0x%02X (held=%s)", elapsed, flow, held_name)
            hold(nil)
            park_since = nil
            -- Keep capturing a little longer so the next flow steps land in
            -- the CSV, then stop.
            if elapsed + 120 < FRAMES then ctx.stop_at = elapsed + 120 end
        end
        if ctx.stop_at and elapsed >= ctx.stop_at then ctx.request_quit = true end

        if (elapsed % 120) == 0 then
            logf("vsync %4d mode=0x%02X flow=0x%02X state=0x%02X timer=%d phase=%d scene=%s held=%s writes=%d",
                elapsed, mode, flow, state, timer,
                probe.read_u8(base + 0x289) or 0, scene_name(), held_name, writes)
        end
    end,

    on_done = function(ctx)
        hold(nil)
        logf("=== stage module (PROT 0968) hits ===")
        local skeys = {}
        for k in pairs(ctx.stage_hits) do skeys[#skeys + 1] = k end
        table.sort(skeys)
        for _, k in ipairs(skeys) do logf("  %s  x%d", k, ctx.stage_hits[k]) end
        logf("=== ctx+6 writes: %d ===", writes)
        local keys = {}
        for k in pairs(ctx.write_sites) do keys[#keys + 1] = k end
        table.sort(keys)
        for _, k in ipairs(keys) do logf("  %s  x%d", k, ctx.write_sites[k]) end
        if csv then csv:close() end
        local f = io.open(OUT_LOG, "w")
        if f then
            f:write(table.concat(lines, "\n") .. "\n")
            f:close()
            PCSX.log("[w4d_cort] wrote " .. OUT_LOG)
        end
    end,
})
