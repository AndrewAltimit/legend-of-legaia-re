-- autorun_gaza2_hpbar_writers.lua
--
-- Names every writer of a PARTY slot's HP-bar triple, by watchpoint rather
-- than by guessing PCs.
--
-- The companion probe autorun_gaza2_acc_discard.lua arms Exec breakpoints on
-- the +0x10 accumulator stores that a grep of the dumped battle corpus finds
-- (FUN_801EC3E4's four sites, FUN_800402F4's three, FUN_80047430's two). A
-- plain observation run showed that census is INCOMPLETE: party slot 0 went
-- through the whole absorbing state - live HP 266 with a displayed bar of 0 and
-- a zero accumulator, held for a dozen vsyncs - while not one of those nine
-- sites ever fired for a party actor. Only the monster slot's stores were seen.
-- So the party side is seeded from somewhere else, and a PC census cannot find
-- it.
--
-- This probe stops guessing. It arms Write watchpoints directly on the three
-- fields of each party actor:
--
--   +0x10   the pending-delta accumulator
--   +0x14C  live HP
--   +0x172  displayed HP
--
-- and logs (vsync, field, slot, pc, ra, prev_value) for every hit, so the
-- writers name themselves. The pairing is what matters: a live-HP write with
-- no accumulator write beside it, or an accumulator write that does not equal
-- the resulting bar-versus-HP gap, is the defect.
--
-- NOTE the logged value is the value the store OVERWROTE, not the one it wrote:
-- PCSX-Redux runs the debug hook before the instruction executes. See the
-- header of probe/watch.lua. Read the resulting value from the next vsync's
-- pairing row instead.
--
-- What the census found: party damage on this save arrives through
-- 0x801E1948 / 0x801E1960 - the SAFE applier, which clamps the damage against
-- live HP and then applies that one clamped value to both the accumulator and
-- live HP - so the party drains are textbook and the invariant holds across
-- them. The nine-site PC census missed it because that applier is in neither
-- FUN_801EC3E4 nor FUN_800402F4.
--
-- The invariant being audited is the one the state-0x51 settle gate
-- (FUN_801E7250) depends on:
--
--     (+0x172 displayed) - (+0x14C live) == (+0x10 accumulator)
--
-- because FUN_80047430's ramp is gated on a non-zero accumulator, so a slot
-- that breaks the invariant and lands on acc == 0 can never converge, and every
-- later action targeting the party side parks at 0x51 with the battle camera
-- orbiting - the community's endless-orbit softlock.
--
-- Actor pointers move when the battle context is rebuilt, so the watchpoints
-- are re-armed whenever a party slot's actor address changes.
--
-- Outputs (under captures/gaza2_hpbar_writers/<ts>/):
--   writes.csv     every hit: vsync, field, slot, pc, ra, prev_value
--   writes.detail.txt  call context for the first hits
--   pairing.csv    per-vsync per-slot hp/bar/acc + invariant + which fields
--                  were written this frame
--   breaks.csv     one row per frame the invariant is broken, with the PCs
--                  that wrote each field that frame
--   summary.txt    writer census + verdict
--
-- Knobs (env):
--   LEGAIA_FRAMES       capture vsyncs (default 6000)
--   LEGAIA_AUTOPILOT    press the next macro button every N vsyncs (0 = off)
--   LEGAIA_AUTOPILOT_SEQ comma-separated button cycle
--   LEGAIA_SSTATE       save state (fingerprint it; never trust a slot number)
--
-- Run:
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_gaza2_hpbar_writers.lua \
--     --scenario battle_gaza2_prompt --frames 6000
--
-- Lua breakpoints need -interpreter -debugger, so never launch this --fast.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad   = require("probe.pad")

local SSTATE    = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES    = probe.getenv_num("LEGAIA_FRAMES", 6000)
local AUTOPILOT = probe.getenv_num("LEGAIA_AUTOPILOT", 0)
local AUTOPILOT_SEQ = probe.getenv("LEGAIA_AUTOPILOT_SEQ",
    "CROSS,DOWN,CROSS,CROSS,CROSS,UP,CROSS,CROSS,RIGHT,CROSS,CROSS,CROSS")

if probe.getenv("LEGAIA_CORE", "") == "dynarec" then
    PCSX.log("[writers] REFUSING --fast launch: Lua breakpoints never fire under the recompiler")
    PCSX.quit(3)
    return
end

local CTX_PTR = 0x8007BD24
local ACTORS  = 0x801C9370

local FIELDS = {
    { off = 0x00,  width = 4, name = "dmg" },   -- per-action damage total
    { off = 0x10,  width = 4, name = "acc" },   -- pending bar delta
    { off = 0x14C, width = 2, name = "hp" },    -- live HP
    { off = 0x172, width = 2, name = "bar" },   -- displayed HP
}

-- The end-of-action live-HP commit inside FUN_801EC3E4, and the guarded exits
-- that skip it.
--
-- The bar accumulator +0x10 and the per-action damage total +0x00 are credited
-- together while the action resolves (0x801EDB40 / 0x801EDB58), but live HP is
-- only written at the END of the action, here:
--
--   801eea10  lhu  a0,0x14c(v1)   ; live HP
--   801eea14  lw   v0,0x0(v1)     ; the action's accumulated damage
--   801eea1c  sltu v0,v0,a0
--   801eea20  bne  v0,zero,0x801eea30
--   801eea2c  _sh  zero,0x14c(v1) ; damage >= HP -> live HP = 0
--   801eea3c  sh   v0,0x14c(v1)   ; else live HP -= damage
--   801eea74  sw   zero,0x0(v1)   ; and the damage total is cleared
--
-- Three guards above it branch to 0x801EEB5C / 0x801EEB60 and skip the commit
-- entirely. A skip that still leaves +0x10 credited moves the bar without
-- moving live HP, which is the `bar < hp` desync direction seen live.
local COMMIT_SITES = {
    { pc = 0x801EEA10, name = "commit_entry" },
    { pc = 0x801EEA2C, name = "commit_overkill_hp0" },
    { pc = 0x801EEA3C, name = "commit_subtract" },
    { pc = 0x801EEB5C, name = "skip_exit_b5c" },
    { pc = 0x801EEB60, name = "skip_exit_b60" },
}

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function i32(a) local v = u32(a); return v >= 0x80000000 and v - 0x100000000 or v end
local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end

local function actor_of(seat)
    local a = u32(ACTORS + seat * 4)
    return in_ram(a) and a or 0
end

local g_elapsed = 0
local armed_for = {}          -- seat -> actor address the watchpoints cover
local writers = {}            -- "field pc" -> count
local this_frame = {}         -- seat -> { field -> pc } written this vsync
local break_frames = 0
local absorb_runs = {}        -- seat -> vsyncs spent absorbing
local absorb_max = {}         -- seat -> longest absorbing run
local last_ctx7 = -1
local commit_armed = false

local writes_csv = probe.csv_open(probe.out_path("writes.csv"),
    "tick,label,addr,pc,ra,prev_value")

local pair_csv = probe.csv_open(probe.out_path("pairing.csv"),
    "vsync,ctx7,seat,hp,bar,acc,invariant,absorbing,wrote_hp_pc,wrote_bar_pc,wrote_acc_pc")

local break_csv = probe.csv_open(probe.out_path("breaks.csv"),
    "vsync,ctx7,seat,hp,bar,acc,invariant,wrote_hp_pc,wrote_bar_pc,wrote_acc_pc")

local watch = require("probe.watch").new{
    csv         = writes_csv,
    detail_path = probe.out_path("writes.detail.txt"),
    max_detail  = 24,
    elapsed     = function() return g_elapsed end,
}

-- The watch helper logs the hit to writes.csv; this wrapper additionally
-- records WHICH field of WHICH slot moved this frame, which is what makes the
-- pairing readable - a live-HP write with no accumulator write beside it is the
-- defect shape, and that is only visible per frame.
local function arm_slot(seat, actor)
    for _, f in ipairs(FIELDS) do
        local label = string.format("s%d_%s", seat, f.name)
        watch:arm(actor + f.off, f.width, label)
        -- A second, cheap breakpoint on the same address purely to tally the
        -- writer PC per (field, slot) and mark the frame. Kept separate from
        -- the watch helper so its CSV contract stays untouched.
        probe.arm_breakpoint(actor + f.off, "Write", f.width,
            string.format("tally_s%d_%s", seat, f.name), function()
                local r = PCSX.getRegisters()
                local pc = bit.band(tonumber(r.pc), 0xFFFFFFFF)
                local key = string.format("%-4s pc=0x%08X", f.name, pc)
                writers[key] = (writers[key] or 0) + 1
                local w = this_frame[seat] or {}
                w[f.name] = pc
                this_frame[seat] = w
            end)
    end
    armed_for[seat] = actor
    PCSX.log(string.format("[writers] armed slot %d at actor 0x%08X", seat, actor))
end

-- Per-action commit tracking: did the end-of-action live-HP commit run, or did
-- a guard skip it while the bar accumulator was already credited?
local commit_counts = {}
local commit_csv = probe.csv_open(probe.out_path("commit.csv"),
    "vsync,ctx7,site,seat,dmg_total,acc,hp,bar")

local function arm_commit_sites()
    for _, s in ipairs(COMMIT_SITES) do
        probe.arm_breakpoint(s.pc, "Exec", 4, s.name, function()
            commit_counts[s.name] = (commit_counts[s.name] or 0) + 1
            local c = u32(CTX_PTR)
            if not in_ram(c) then return end
            local ctx7 = u8(c + 7)
            -- Log every party slot's triple at the commit boundary; which slot
            -- the arm is acting on is register-dependent, and the whole party
            -- picture is what the pairing question needs anyway.
            for seat = 0, 2 do
                local a = actor_of(seat)
                if a ~= 0 then
                    commit_csv:row("%d,0x%02X,%s,%d,%d,%d,%d,%d",
                        g_elapsed, ctx7, s.name, seat,
                        i32(a + 0x00), i32(a + 0x10),
                        u16(a + 0x14C), u16(a + 0x172))
                end
            end
        end)
    end
    PCSX.log(string.format("[writers] armed %d commit sites", #COMMIT_SITES))
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

probe.run{
    sstate         = SSTATE,
    capture_frames = FRAMES,
    boot_delay     = 60,
    on_arm         = function() return { "deferred" } end,
    on_capture     = function(_, v)
        g_elapsed = v

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
        local ctx7 = u8(c + 7)
        last_ctx7 = ctx7

        if not commit_armed then
            arm_commit_sites()
            commit_armed = true
        end

        local party = u8(c + 0x00)
        if party < 1 or party > 3 then party = 3 end

        for seat = 0, party - 1 do
            local a = actor_of(seat)
            if a ~= 0 and armed_for[seat] ~= a then arm_slot(seat, a) end
            if a ~= 0 then
                local hp, bar, acc = u16(a + 0x14C), u16(a + 0x172), i32(a + 0x10)
                local inv = (bar - hp) - acc
                local absorbing = (hp ~= bar) and (acc == 0)

                if absorbing then
                    absorb_runs[seat] = (absorb_runs[seat] or 0) + 1
                    if (absorb_runs[seat] or 0) > (absorb_max[seat] or 0) then
                        absorb_max[seat] = absorb_runs[seat]
                    end
                else
                    absorb_runs[seat] = 0
                end

                local w = this_frame[seat] or {}
                local hp_pc  = w.hp  and string.format("0x%08X", w.hp)  or ""
                local bar_pc = w.bar and string.format("0x%08X", w.bar) or ""
                local acc_pc = w.acc and string.format("0x%08X", w.acc) or ""

                if w.hp or w.bar or w.acc or absorbing then
                    pair_csv:row("%d,0x%02X,%d,%d,%d,%d,%d,%s,%s,%s,%s",
                        v, ctx7, seat, hp, bar, acc, inv,
                        absorbing and "yes" or "no", hp_pc, bar_pc, acc_pc)
                end

                -- A live-HP write with no accumulator write beside it is the
                -- defect shape: the bar has been told nothing about the change.
                if inv ~= 0 and (w.hp or w.bar or w.acc) then
                    break_frames = break_frames + 1
                    break_csv:row("%d,0x%02X,%d,%d,%d,%d,%d,%s,%s,%s",
                        v, ctx7, seat, hp, bar, acc, inv, hp_pc, bar_pc, acc_pc)
                end
                this_frame[seat] = nil
            end
        end
    end,
    on_done = function()
        local lines = {}
        local function add(f, ...) lines[#lines + 1] = string.format(f, ...) end
        add("=== gaza2 HP-bar writer census ===")
        add("vsyncs captured: %d   total watchpoint hits: %d", g_elapsed, watch:total())
        add("")
        add("-- writers by (field, pc); see writes.csv for the full log --")
        local keys = {}
        for k in pairs(writers) do keys[#keys + 1] = k end
        table.sort(keys)
        for _, k in ipairs(keys) do add("  %-28s hits=%d", k, writers[k]) end
        if #keys == 0 then
            add("  (the per-frame tally is in pairing.csv; writes.csv holds every hit)")
        end
        add("")
        add("-- end-of-action live-HP commit (FUN_801EC3E4) --")
        for _, s in ipairs(COMMIT_SITES) do
            add("  %-22s hits=%d", s.name, commit_counts[s.name] or 0)
        end
        add("`commit_entry` is only reached when all three guards pass, so it")
        add("counts commits TAKEN and should equal subtract + overkill. The two")
        add("skip exits are shared with the function's ordinary epilogue, so")
        add("their counts are upper bounds on skips, not skip counts.")
        add("")
        add("-- invariant --")
        add("frames with a broken invariant on a written slot: %d", break_frames)
        for seat = 0, 2 do
            add("  slot %d longest absorbing run: %d vsyncs",
                seat, absorb_max[seat] or 0)
        end
        add("")
        add("An absorbing run of more than a few vsyncs is the softlock condition:")
        add("live HP and the displayed bar disagree while the accumulator that")
        add("would close the gap reads zero, so FUN_80047430's ramp never runs and")
        add("FUN_801E7250 answers 'not settled' for the rest of the battle.")
        probe.write_snapshot(probe.out_path("summary.txt"), table.concat(lines, "\n"))
        for _, l in ipairs(lines) do PCSX.log("[writers] " .. l) end
    end,
}
