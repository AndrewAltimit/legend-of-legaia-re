-- autorun_slippery_budget_gate.lua
--
-- When does SCUS's one call into slot B at a fixed VA fire? The per-actor
-- battle draw tick `FUN_800480D8` ends its battle-end teardown preamble with
-- `jal 0x801F7B88` at 0x800481A0, gated on `_DAT_8007BDC0 != 0` - the
-- effect-drain budget PROT 0920 (`cast_slippery`) seeds, drains and clears.
-- The preamble itself is reached only when the actor is live, the teardown
-- request byte `gp[+0xA0C]->[+0x272]` is non-zero and the battle-end signal
-- `_DAT_8007BD71` reads 0xFF, so the call needs a battle that ends while a
-- Slippery cast is still draining.
--
-- The probe records the whole ordering: every write to the budget (with the
-- writing PC), every entry into the teardown preamble, every firing of the
-- call, and the loader-B tracker `0x8007BC4C` (= extraction - 895) at each,
-- so the call can be paired with the image actually resident at slot B.
--
-- Outputs (LEGAIA_OUT_DIR): budget_writes.csv, gate_events.csv, summary.txt.
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_MAX_ROWS.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE   = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1800)
local MAX_ROWS = probe.getenv_num("LEGAIA_MAX_ROWS", 3000)

local BUDGET    = 0x8007BDC0   -- gp[+0xAA8], PROT 0920's effect-drain budget
local PREAMBLE  = 0x8004818C   -- the gate read at the end of the teardown arm
local CALLSITE  = 0x800481A0   -- jal 0x801F7B88
local TRACKER   = 0x8007BC4C   -- loader-B tracker (extraction - 895)
local ENDSIGNAL = 0x8007BD71   -- battle-end signal byte
local CTXPTR    = 0x8007BD24   -- gp[+0xA0C], the battle context pointer
local MODE_VA   = 0x8007B83C

local writes, events = {}, {}
local n_write, n_preamble, n_call = 0, 0, 0
local budget_max, budget_nonzero_frames = 0, 0
local vsync = 0

local function tracker() return probe.read_u32(TRACKER) or 0 end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        probe.bp.arm(BUDGET, "Write", 4, "budget_write", function()
            n_write = n_write + 1
            if #writes >= MAX_ROWS then return end
            local r = PCSX.getRegisters()
            writes[#writes + 1] = string.format("%d,0x%08X,%d,%d",
                vsync, bit.band(tonumber(r.pc), 0xFFFFFFFF),
                probe.read_u32(BUDGET) or 0, tracker())
        end)
        probe.bp.arm(PREAMBLE, "Exec", 4, "teardown_preamble", function()
            n_preamble = n_preamble + 1
            if #events >= MAX_ROWS then return end
            events[#events + 1] = string.format("%d,preamble,%d,%d,%d",
                vsync, probe.read_u32(BUDGET) or 0, tracker(),
                probe.read_u8(ENDSIGNAL) or 0)
        end)
        probe.bp.arm(CALLSITE, "Exec", 4, "slotb_call", function()
            n_call = n_call + 1
            if #events >= MAX_ROWS then return end
            events[#events + 1] = string.format("%d,call,%d,%d,%d",
                vsync, probe.read_u32(BUDGET) or 0, tracker(),
                probe.read_u8(ENDSIGNAL) or 0)
        end)
        return {}
    end,

    on_capture = function(_, elapsed)
        vsync = elapsed
        local b = probe.read_u32(BUDGET) or 0
        if b ~= 0 then
            budget_nonzero_frames = budget_nonzero_frames + 1
            if b > budget_max then budget_max = b end
        end
        if elapsed % 120 ~= 0 then return end
        local ctx = probe.read_u32(CTXPTR) or 0
        local req = 0
        if probe.in_ram(ctx + 0x272, 1) then req = probe.read_u8(ctx + 0x272) or 0 end
        PCSX.log(string.format(
            "[slippery] vsync=%d mode=0x%02X budget=%d tracker=%d end=0x%02X"
            .. " req=%d writes=%d preamble=%d call=%d",
            elapsed, probe.read_u8(MODE_VA) or 0, b, tracker(),
            probe.read_u8(ENDSIGNAL) or 0, req,
            n_write, n_preamble, n_call))
    end,

    on_done = function()
        local fh = io.open(probe.out_path("budget_writes.csv"), "w")
        if fh then
            fh:write("vsync,pc,value_after,tracker\n")
            for _, r in ipairs(writes) do fh:write(r .. "\n") end
            fh:close()
        end
        local eh = io.open(probe.out_path("gate_events.csv"), "w")
        if eh then
            eh:write("vsync,event,budget,tracker,end_signal\n")
            for _, r in ipairs(events) do eh:write(r .. "\n") end
            eh:close()
        end
        local sh = io.open(probe.out_path("summary.txt"), "w")
        if sh then
            sh:write(string.format("vsyncs=%d\n", vsync))
            sh:write(string.format("budget_writes=%d\n", n_write))
            sh:write(string.format("budget_max=%d\n", budget_max))
            sh:write(string.format("budget_nonzero_frames=%d\n",
                budget_nonzero_frames))
            sh:write(string.format("teardown_preamble_entries=%d\n", n_preamble))
            sh:write(string.format("slotb_calls=%d\n", n_call))
            sh:write(string.format("final_tracker=%d\n", tracker()))
            sh:close()
        end
        PCSX.log(string.format(
            "[slippery] done: writes=%d max=%d nonzero_frames=%d preamble=%d call=%d",
            n_write, budget_max, budget_nonzero_frames, n_preamble, n_call))
    end,
})
