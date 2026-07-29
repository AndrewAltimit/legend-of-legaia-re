-- autorun_grid_far_colour.lua
--
-- Pins the GTE far colour (FC, control regs 21-23 = RFC/GFC/BFC) the battle
-- ground-grid emitter `func_0x801d02c0` draws with. The emitter itself
-- contains zero `ctc2` writes (see
-- ghidra/scripts/funcs/overlay_battle_action_801d02c0.txt), so the FC its
-- four `DPCS` sites (0x801d061c / 063c / 0654 / 0688) consume is whatever
-- the control file holds on entry - and a save-state read cannot attribute
-- that value to the grid pass (the snapshot holds whatever the LAST GTE
-- setup staged, as likely the UI pass as the grid). An exec breakpoint on
-- the emitter attributes it by construction.
--
-- Arms two Exec breakpoints:
--   * 0x801D02C0 - emitter entry (FC staged by the caller, pre-draw)
--   * 0x801D061C - the first DPCS site (FC at the exact consuming op)
--
-- Each hit logs RFC/GFC/BFC plus the cross-checks: DQA/DQB (cr27/cr28, the
-- already-pinned depth-cue pair), the staged far-colour word at 0x8007BB48
-- (the backdrop far-colour derivation target, battle.md), and the stage id
-- word 0x80084540.
--
-- Run from the repo root against a battle save state (the grid only draws
-- in mode-0x15 battle frames; a field state is the negative control - the
-- breakpoints must stay silent):
--
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_grid_far_colour.lua \
--   LEGAIA_OUT=/tmp/grid_fc_queenbee.csv LEGAIA_FRAMES=240 \
--       timeout --kill-after=10s 300 \
--       bash scripts/pcsx-redux/run_probe.sh --scenario rim_elm_queen_bee_battle
--
-- Env vars: the usual harness set (LEGAIA_SSTATE / LEGAIA_FRAMES /
-- LEGAIA_OUT), plus LEGAIA_MAX_ROWS (CSV row cap, default 48) and
-- LEGAIA_ENTRY_QUOTA (emitter-entry hits before early quit, default 8 -
-- raise it to watch whether FC settles or ramps across the battle intro).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 240)
local OUT_PATH = probe.out_path("grid_fc.csv")
local MAX_ROWS = probe.getenv_num("LEGAIA_MAX_ROWS", 48)
local ENTRY_QUOTA = probe.getenv_num("LEGAIA_ENTRY_QUOTA", 8)
local SSTATE   = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")

local GRID_ENTRY = 0x801D02C0 -- func_0x801d02c0 (battle ground-grid emitter)
local GRID_DPCS  = 0x801D061C -- first of its four DPCS (cop2 0x780010) sites
local FAR_STAGE  = 0x8007BB48 -- backdrop far-colour staging word (battle.md)
local STAGE_ID   = 0x80084540 -- backdrop stage id word

local csv = probe.csv_open(OUT_PATH,
    "site,hit,rfc,gfc,bfc,dqa,dqb,far_stage_word,stage_id_word")

local rows = 0
local hits = { entry = 0, dpcs = 0 }

local function n32(v) return bit.band(v, 0xFFFFFFFF) end

local function log_site(site, key)
    hits[key] = hits[key] + 1
    -- The DPCS site fires 4x per visible cell per frame; 16 rows of it is
    -- plenty of confirmation. Entry rows (one per frame) keep logging so a
    -- long run shows whether FC settles or ramps across the battle intro.
    if key == "dpcs" and hits[key] > 16 then return end
    if rows >= MAX_ROWS then return end
    rows = rows + 1
    local r = PCSX.getRegisters()
    -- ffi arrays are 0-indexed: CP2C.r[21] IS control register 21 (RFC).
    local rfc = n32(r.CP2C.r[21])
    local gfc = n32(r.CP2C.r[22])
    local bfc = n32(r.CP2C.r[23])
    local dqa = n32(r.CP2C.r[27])
    local dqb = n32(r.CP2C.r[28])
    csv:row("%s,%d,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X",
        site, hits[key], rfc, gfc, bfc, dqa, dqb,
        n32(probe.read_u32(FAR_STAGE) or 0),
        n32(probe.read_u32(STAGE_ID) or 0))
    if rows <= 6 then
        PCSX.log(string.format(
            "[grid_fc] %s hit %d: FC=(0x%X,0x%X,0x%X) DQA=0x%X DQB=0x%X",
            site, hits[key], rfc, gfc, bfc, dqa, dqb))
    end
end

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.arm_breakpoint(GRID_ENTRY, "Exec", 4, "grid_entry",
            function() log_site("entry", "entry") end)
        probe.arm_breakpoint(GRID_DPCS, "Exec", 4, "grid_dpcs",
            function() log_site("dpcs", "dpcs") end)
        PCSX.log(string.format(
            "[grid_fc] armed entry=0x%08X dpcs=0x%08X max_rows=%d",
            GRID_ENTRY, GRID_DPCS, MAX_ROWS))
        return {}
    end,
    on_capture = function(ctx, _elapsed)
        -- Enough attribution: several whole-frame entries plus DPCS-site
        -- confirmations. Bail early rather than run the full budget.
        if hits.entry >= ENTRY_QUOTA and hits.dpcs >= 16 then
            ctx.request_quit = true
        end
    end,
    on_done = function()
        csv:close()
        PCSX.log("=== summary ===")
        PCSX.log(string.format(
            "[grid_fc] entry_hits=%d dpcs_hits=%d rows=%d out=%s",
            hits.entry, hits.dpcs, rows, OUT_PATH))
    end,
})
