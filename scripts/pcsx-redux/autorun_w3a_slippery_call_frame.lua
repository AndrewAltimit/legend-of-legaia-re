-- autorun_w3a_slippery_call_frame.lua
--
-- When does SCUS's one call into slot B (`jal 0x801F7B88` at `0x800481A0`,
-- inside the per-actor battle draw tick `FUN_800480D8`) actually fire?
--
-- The gate is `_DAT_8007BDC0 != 0`, and that word is PROT 0920's
-- (`cast_slippery`) own effect-drain budget: 0920 seeds it to `0x204`, drains
-- it per frame and clears it, and no other image in SCUS or the 83 mapped
-- overlays writes it. `autorun_slippery_budget_gate.lua` measured the arm on a
-- victory ladder and found the budget zero at all 123 gate entries - because
-- no PROT-Redux state in the corpus has a Slippery cast running
-- (`slippery_summon_mid_cast` is a mednafen backup, and mednafen has no
-- breakpoints).
--
-- This probe closes that by *driving* the cast instead of resuming into one:
-- it resumes an ordinary pre-cast battle state, rewrites the acting party
-- seat's queued action into Slippery (spell `0x92` -> extraction 920, on the
-- `extraction = spell_id - 0x79 + 895` arithmetic the mid-cast corpus pins),
-- and watches the budget and the gate across the whole cast.
--
-- Rows: every write to `_DAT_8007BDC0` with the writing PC, every entry into
-- the gate read, every firing of the call, the loader-B tracker `0x8007BC4C`
-- so a firing is paired with the image resident at slot B, and the battle-end
-- signal `_DAT_8007BD71` at each.
--
-- Outputs (LEGAIA_OUT_DIR): budget_writes.csv, gate_events.csv, summary.txt.
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_SPELL (default 0x92),
--      LEGAIA_TARGET_SEAT, LEGAIA_CASTER_SEAT, LEGAIA_INJECT_STATE,
--      LEGAIA_MP_TOPUP, LEGAIA_MAX_ROWS.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 2600)
local MAX_ROWS = probe.getenv_num("LEGAIA_MAX_ROWS", 4000)
local SPELL    = probe.getenv_num("LEGAIA_SPELL", 0x92)
local TARGET   = probe.getenv_num("LEGAIA_TARGET_SEAT", 3)
local CASTER   = probe.getenv_num("LEGAIA_CASTER_SEAT", 0)
local MP_TOPUP = probe.getenv_num("LEGAIA_MP_TOPUP", 999)

local BUDGET    = 0x8007BDC0   -- gp[+0xAA8], PROT 0920's effect-drain budget
local PREAMBLE  = 0x8004818C   -- the gate read at the end of the teardown arm
local CALLSITE  = 0x800481A0   -- jal 0x801F7B88
local CALLEE    = 0x801F7B88   -- the slot-B routine itself
local TRACKER   = 0x8007BC4C   -- loader-B tracker (extraction - 895)
local ENDSIGNAL = 0x8007BD71   -- battle-end signal byte
local CTXPTR    = 0x8007BD24
local ACTORS    = 0x801C9370

local function u8(a) return probe.read_u8(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function ctxp()
    local c = u32(CTXPTR)
    if c < 0x80000000 or c >= 0x80200000 then return nil end
    return c
end
local function actor(s)
    local p = u32(ACTORS + s * 4)
    if p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end
local function pc_now()
    local ok, r = pcall(PCSX.getRegisters)
    if not ok or not r then return 0 end
    local v = tonumber(r.pc) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end

local writes, events = nil, nil
local n_write, n_preamble, n_call, n_callee = 0, 0, 0, 0
local budget_max = 0
local vsync, injected, cast_seen = 0, false, false
local inject_vsync, first_call_vsync, first_call_phase = -1, -1, -1

probe.run({
    sstate = SSTATE, capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_w3a_slippery_call_frame.lua", {
            sstate = SSTATE, spell = string.format("0x%02X", SPELL),
            target_seat = TARGET, caster_seat = CASTER, frames = FRAMES,
        })
        writes = probe.csv_open(probe.out_path("budget_writes.csv"),
            "vsync,pc,budget,tracker,phase")
        events = probe.csv_open(probe.out_path("gate_events.csv"),
            "vsync,kind,budget,tracker,endsignal,phase")

        probe.bp.arm(BUDGET, "Write", 4, "budget_write", function()
            n_write = n_write + 1
            local b = u32(BUDGET)
            if b > budget_max then budget_max = b end
            if n_write <= MAX_ROWS then
                local cx = ctxp()
                writes:row("%d,0x%08X,%d,%d,%d", vsync, pc_now(), b,
                    u32(TRACKER), cx and u8(cx + 0x279) or -1)
            end
        end)
        probe.bp.arm(PREAMBLE, "Exec", 4, "gate_read", function()
            n_preamble = n_preamble + 1
            if n_preamble <= MAX_ROWS then
                local cx = ctxp()
                events:row("%d,gate,%d,%d,%d,%d", vsync, u32(BUDGET),
                    u32(TRACKER), u8(ENDSIGNAL), cx and u8(cx + 0x279) or -1)
            end
        end)
        probe.bp.arm(CALLSITE, "Exec", 4, "callsite", function()
            n_call = n_call + 1
            local cx = ctxp()
            if first_call_vsync < 0 then
                first_call_vsync = vsync
                first_call_phase = cx and u8(cx + 0x279) or -1
                PCSX.log(string.format(
                    "[CALL t%d] jal 0x801F7B88 FIRED budget=%d tracker=%d end=0x%02X phase=%d",
                    vsync, u32(BUDGET), u32(TRACKER), u8(ENDSIGNAL), first_call_phase))
            end
            if n_call <= MAX_ROWS then
                events:row("%d,call,%d,%d,%d,%d", vsync, u32(BUDGET),
                    u32(TRACKER), u8(ENDSIGNAL), cx and u8(cx + 0x279) or -1)
            end
        end)
        probe.bp.arm(CALLEE, "Exec", 4, "callee", function()
            n_callee = n_callee + 1
            if n_callee <= MAX_ROWS then
                local cx = ctxp()
                events:row("%d,callee,%d,%d,%d,%d", vsync, u32(BUDGET),
                    u32(TRACKER), u8(ENDSIGNAL), cx and u8(cx + 0x279) or -1)
            end
        end)
        return {}
    end,

    on_capture = function(c, elapsed)
        vsync = elapsed
        local cx = ctxp()
        local cs = actor(CASTER)
        if cx == nil or cs == nil then return end
        probe.pad_release(probe.BTN.CROSS)
        if not injected and elapsed < 300 then
            local sub = elapsed % 60
            if sub >= 30 and sub < 34 then probe.pad_force(probe.BTN.CROSS) end
        end
        local st7 = u8(cx + 7)
        if not injected and st7 == 0x0A and u8(cx + 0x13) == CASTER then
            injected = true
            inject_vsync = elapsed
            if MP_TOPUP > 0 then probe.write_u16(cs + 0x150, MP_TOPUP) end
            probe.write_u8(cs + 0x1DE, 2)
            probe.write_u8(cs + 0x1DF, SPELL)
            probe.write_u8(cs + 0x1DD, TARGET)
            PCSX.log(string.format("[inject t%d] spell=0x%02X target=%d", elapsed, SPELL, TARGET))
        end
        if injected and not cast_seen and st7 >= 0x32 and st7 <= 0x38 then
            cast_seen = true
            PCSX.log(string.format("[cast t%d] summon band st7=0x%02X tracker=%d",
                elapsed, st7, u32(TRACKER)))
        end
    end,

    on_done = function()
        local fh = io.open(probe.out_path("summary.txt"), "w")
        local txt = string.format(
            "inject_vsync=%d cast_entered=%s\nbudget_writes=%d budget_max=%d\n" ..
            "gate_reads=%d callsite_hits=%d callee_hits=%d\n" ..
            "first_call_vsync=%d first_call_module_phase=%d\n",
            inject_vsync, tostring(cast_seen), n_write, budget_max,
            n_preamble, n_call, n_callee, first_call_vsync, first_call_phase)
        if fh then fh:write(txt); fh:close() end
        PCSX.log("[w3a-slippery] " .. txt:gsub("\n", " | "))
        if writes then writes:close() end
        if events then events:close() end
    end,
})
