-- autorun_gaza2_acc_discard.lua
--
-- Where the Gaza 2 "endless camera orbit" desync is BORN.
--
-- The park itself is already pinned: battle-action state 0x51 exits only when
-- FUN_801E7250 answers "settled", and that check answers "not settled" while a
-- party slot's live HP (+0x14C) disagrees with its DISPLAYED HP (+0x172). The
-- two converge through a third field: actor+0x10, a pending-delta accumulator
-- that FUN_80047430 quarter-steps into the bar every frame - and only while it
-- is non-zero (`lw a0,0x10(s2); beq a0,zero,<skip>` at 0x800474E8). So the
-- absorbing state is:
--
--     +0x14C != +0x172   AND   +0x10 == 0
--
-- from which nothing in the corpus recovers except FUN_801E752C's status-tick
-- re-sync. What was NOT pinned is what puts a party slot INTO that state.
--
-- This probe measures the invariant that has to hold for the bar to land:
--
--     (+0x172 - +0x14C) == +0x10        [bar lag equals pending delta]
--
-- and attributes every break to the store that caused it, by arming an Exec
-- breakpoint on each writer of actor+0x10 in the corpus. Two conventions
-- coexist there, which is the whole point:
--
--   ACCUMULATE - the battle damage/heal kernel FUN_801EC3E4 reads, adds, and
--     writes back at every one of its sites (0x801EDAF0 overheal-clamped,
--     0x801EDB14 net-delta, 0x801EDB58 second-actor, 0x801EDB7C anti-overkill
--     clamp). Overlapping hits compose correctly and the invariant survives.
--
--   ASSIGN - the item / restore applier FUN_800402F4 does a bare
--     `sw v0,0x10(v1)` with v0 = -amount at ALL THREE of its sites
--     (0x800408FC, 0x80040D28, 0x800410BC). It never reads the old value, so a
--     restore that lands while a damage drain is still in flight DISCARDS the
--     remainder. The bar then settles `remainder` away from live HP with the
--     accumulator at zero - precisely the absorbing state above.
--
-- So the predicted retail sequence is: Gaza hits a party member (bar starts a
-- multi-frame drain) -> the player heals that member before the bar lands ->
-- the remainder is discarded -> every later action whose acting actor targets
-- that slot (+0x1DD in 0..2) or targets the whole party (+0x1DD == 8, which is
-- what a party-wide spell uses) parks at 0x51 while FUN_801D0748's
-- unconditional yaw sweep keeps orbiting the camera.
--
-- The probe does not have to make the softlock happen to settle the question.
-- ONE observed hit at an ASSIGN site with a non-zero prior accumulator proves
-- retail reaches the discard; the arithmetic after that is not in doubt. So
-- this runs in pure observation by default - no HP writes, no godmode, nothing
-- that could manufacture its own result (the previous wave's "reproduction"
-- turned out to be its own godmode clamp, which is why that matters here).
--
-- Outputs (under captures/gaza2_acc_discard/<ts>/):
--   acc_writes.csv  every actor+0x10 store: pc, convention, slot, prior, new
--   discards.csv    the subset where an ASSIGN overwrote a non-zero prior
--   invariant.csv   per-vsync per-party-slot hp/bar/acc and the invariant
--   absorbing.csv   transitions into and out of (hp != bar && acc == 0)
--   final_heal.csv  each FUN_801E6968 revive call: seat + acc at the call
--   settle.csv      each 0x51 settle-check verdict (v0 at 0x801E604C) + ctx+0x6D8
--   summary.txt     terse verdict
--
-- The strongest deliberate driver of the discard is the Lost Grail Final Heal
-- (state 0x50 runs FUN_801E6968, whose class-4 revive call reaches the bare
-- assign at 0x800410BC on the very action whose killing hit credited the
-- accumulator). The save's inventory owns exactly one Lost Grail (item 0xE7)
-- but nobody wears it, so LEGAIA_EQUIP_GRAIL pokes it into a party member's
-- accessory slot 7 (record +0x19D) while the battle is still parked at the
-- Begin/Run prompt. That reproduces a configuration ordinary play reaches by
-- equipping the owned grail in the pause menu before the fight; the per-frame
-- stat aggregator FUN_80042558 then derives ability bit 0x27 (record +0xF8
-- bit 7 - the exact gate the Final Heal checks) from the equipment id, and
-- every HP-side write still flows through retail code only. The poke touches
-- no HP field, no bar field, no accumulator.
--
-- Knobs (env):
--   LEGAIA_FRAMES        capture vsyncs (default 4000)
--   LEGAIA_AUTOPILOT     press the next macro button every N vsyncs (0 = off)
--   LEGAIA_AUTOPILOT_SEQ comma-separated button cycle
--   LEGAIA_EQUIP_GRAIL   party seat 0..2 whose accessory slot 7 becomes the
--                        Lost Grail (0xE7) at vsync 60, or -1 = no poke
--                        (default -1)
--   LEGAIA_SSTATE        save state (fingerprint it; never trust a slot number)
--
-- Run:
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_gaza2_acc_discard.lua \
--     --scenario battle_gaza2_prompt --frames 4000
--
-- Lua breakpoints need -interpreter -debugger, so never launch this --fast.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad   = require("probe.pad")

local SSTATE      = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 4000)
local AUTOPILOT   = probe.getenv_num("LEGAIA_AUTOPILOT", 0)
local EQUIP_GRAIL = probe.getenv_num("LEGAIA_EQUIP_GRAIL", -1)
local AUTOPILOT_SEQ = probe.getenv("LEGAIA_AUTOPILOT_SEQ",
    "CROSS,DOWN,CROSS,CROSS,CROSS,UP,CROSS,CROSS,RIGHT,CROSS,CROSS,CROSS")

if probe.getenv("LEGAIA_CORE", "") == "dynarec" then
    PCSX.log("[discard] REFUSING --fast launch: Lua breakpoints never fire under the recompiler")
    PCSX.quit(3)
    return
end

local CTX_PTR = 0x8007BD24
local ACTORS  = 0x801C9370
local CAM_YAW = 0x8007B792

-- Character records (the pause-menu-side structs, not the battle actors).
local PARTY_ID_TBL = 0x8007BD10 -- seat -> 1-based party id
local CHAR_REC     = 0x80084708 -- + (party_id-1)*0x414
local LOST_GRAIL   = 0xE7       -- item id; passive index 0x27 = +0xF8 bit 7

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

-- Reverse the actor-pointer table so a store's target register can be named as
-- a battle seat rather than a bare address.
local function seat_of(actor)
    if actor == 0 then return -1 end
    for s = 0, 7 do
        if actor_of(s) == actor then return s end
    end
    return -1
end

------------------------------------------------------------------
-- Every writer of actor+0x10 in the corpus.
--
-- `bp` is the address the Exec breakpoint is armed on, `store` the address of
-- the `sw` it stands for. Where the store sits in a branch delay slot the
-- breakpoint is armed on the branch instead: both registers are already
-- computed by then, and arming a delay slot is not worth the risk of the
-- interpreter's branch handling skipping the hit.
--
-- `ptr` names the register holding the actor base, `val` the register holding
-- the value about to be stored ("zero" and "clamp" are handled specially).
local WRITERS = {
    -- FUN_800402F4, the item / restore applier: bare assign, discards the
    -- in-flight remainder. These are the suspects.
    { bp = 0x800408F8, store = 0x800408FC, kind = "ASSIGN",
      ptr = "v1", val = "v0", who = "FUN_800402F4 restore site A" },
    { bp = 0x80040D28, store = 0x80040D28, kind = "ASSIGN",
      ptr = "v1", val = "v0", who = "FUN_800402F4 restore site B" },
    { bp = 0x800410B8, store = 0x800410BC, kind = "ASSIGN",
      ptr = "v1", val = "v0", who = "FUN_800402F4 restore site C" },

    -- FUN_801EC3E4, the battle damage/heal kernel: read-modify-write at every
    -- site, so overlapping hits compose and the invariant survives.
    { bp = 0x801EDAEC, store = 0x801EDAF0, kind = "ACCUM",
      ptr = "a0", val = "v0", who = "FUN_801EC3E4 overheal-clamped" },
    { bp = 0x801EDB14, store = 0x801EDB14, kind = "ACCUM",
      ptr = "v1", val = "v0", who = "FUN_801EC3E4 net-delta" },
    { bp = 0x801EDB58, store = 0x801EDB58, kind = "ACCUM",
      ptr = "v1", val = "v0", who = "FUN_801EC3E4 second-actor" },
    { bp = 0x801EDB7C, store = 0x801EDB7C, kind = "CLAMP",
      ptr = "v1", val = "a1", who = "FUN_801EC3E4 anti-overkill clamp" },

    -- The enemy-cast damage applier inside the cast tick FUN_801E09F8 - the
    -- SAFE shape (clamps the damage once, applies it to both fields). This is
    -- how Gaza's own casts credit a party accumulator, so without it every
    -- party-side blame reads "(none seen)".
    { bp = 0x801E1948, store = 0x801E1948, kind = "ACCUM",
      ptr = "v1", val = "v0", who = "FUN_801E09F8 enemy-cast safe applier" },

    -- FUN_80047430, the per-actor bar tick. The party arm writes back the
    -- quarter-stepped remainder; the non-party arm zeroes the accumulator and
    -- applies the whole delta in one frame.
    { bp = 0x80047570, store = 0x80047574, kind = "DRAIN",
      ptr = "s2", val = "v0", who = "FUN_80047430 party quarter-step" },
    { bp = 0x80047580, store = 0x80047580, kind = "DRAIN0",
      ptr = "s2", val = "zero", who = "FUN_80047430 non-party one-shot" },
}

-- The two Final Heal call sites inside FUN_801E6968 (state 0x50). Each is
-- `jal 0x800402F4` with a0=4 (revive class), a1=1 (full); a2 carries the
-- seat for the single-target site. Logging them separates "the auto-revive
-- fired" from "a menu Phoenix fired" at the same applier.
local FINAL_HEAL_SITES = {
    { bp = 0x801E6A24, who = "FUN_801E6968 single-target revive" },
    { bp = 0x801E6BD0, who = "FUN_801E6968 sweep revive" },
}

-- The 0x51 arm's settle-check return: 801e6044 jal 0x801e7250, so the
-- delay-slot-adjusted return lands at 0x801E604C where v0 IS the verdict
-- (non-zero = "not settled" = the countdown decrement is skipped).
local SETTLE_RET = 0x801E604C

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

local write_counts = {}       -- store address -> hit count
local discards = {}           -- list of discard events
local absorbing_since = {}    -- seat -> vsync it entered the absorbing state
local absorbing_ever = {}     -- seat -> true if it was ever absorbing
local last_inv = {}           -- seat -> last invariant value
local last_acc_writer = {}    -- seat -> {pc, who} of the most recent +0x10 store
local park_vsync = nil
local final_heals = {}        -- list of Final Heal firings
local settle_counts = { settled = 0, blocked = 0 }
local grail_poked_at = nil
local grail_bit_seen = nil    -- vsync the +0xF8 bit-7 first read set

local acc_csv = probe.csv_open(probe.out_path("acc_writes.csv"),
    "vsync,store_pc,kind,who,seat,actor,prior_acc,new_acc,hp,bar,bar_minus_hp,discarded")

local disc_csv = probe.csv_open(probe.out_path("discards.csv"),
    "vsync,store_pc,who,seat,prior_acc,new_acc,hp,bar,bar_minus_hp")

local inv_csv = probe.csv_open(probe.out_path("invariant.csv"),
    "vsync,ctx7,seat,hp,bar,acc,invariant,absorbing,last_writer")

local abs_csv = probe.csv_open(probe.out_path("absorbing.csv"),
    "vsync,seat,event,hp,bar,acc,residual,blamed_pc,blamed_who")

local heal_csv = probe.csv_open(probe.out_path("final_heal.csv"),
    "vsync,site_pc,who,a2_seat,hp,bar,acc")

local settle_csv = probe.csv_open(probe.out_path("settle.csv"),
    "vsync,verdict,ctx6d8,acting_seat,target_1dd")

local function note_absorbing(seat, event, hp, bar, acc)
    local blame = last_acc_writer[seat] or { pc = 0, who = "(none seen)" }
    abs_csv:row("%d,%d,%s,%d,%d,%d,%d,0x%08X,%s",
        g_elapsed, seat, event, hp, bar, acc, bar - hp, blame.pc, blame.who)
end

local function on_acc_write(w)
    return function()
        local r = PCSX.getRegisters()
        local actor = tonumber(r.GPR.n[w.ptr]) or 0
        if not in_ram(actor) then return end

        local new_acc
        if w.val == "zero" then
            new_acc = 0
        else
            new_acc = tonumber(r.GPR.n[w.val]) or 0
            if new_acc >= 0x80000000 then new_acc = new_acc - 0x100000000 end
        end

        local prior = i32(actor + 0x10)
        local hp, bar = u16(actor + 0x14C), u16(actor + 0x172)
        local seat = seat_of(actor)

        write_counts[w.store] = (write_counts[w.store] or 0) + 1
        if seat >= 0 then last_acc_writer[seat] = { pc = w.store, who = w.who } end

        -- The decisive event: a bare assign landing on a non-zero accumulator.
        -- Whatever was left of the previous drain is gone, and the bar will
        -- settle exactly `prior` away from live HP.
        local discarded = 0
        if w.kind == "ASSIGN" and prior ~= 0 then
            discarded = prior
            discards[#discards + 1] = {
                vsync = g_elapsed, pc = w.store, who = w.who, seat = seat,
                prior = prior, new_acc = new_acc, hp = hp, bar = bar,
            }
            disc_csv:row("%d,0x%08X,%s,%d,%d,%d,%d,%d,%d",
                g_elapsed, w.store, w.who, seat, prior, new_acc, hp, bar, bar - hp)
            PCSX.log(string.format(
                "[discard] vsync=%d %s (0x%08X) seat=%d DISCARDED remainder %d " ..
                "(acc %d -> %d, hp=%d bar=%d)",
                g_elapsed, w.who, w.store, seat, prior, prior, new_acc, hp, bar))
        end

        acc_csv:row("%d,0x%08X,%s,%s,%d,0x%08X,%d,%d,%d,%d,%d,%d",
            g_elapsed, w.store, w.kind, w.who, seat, actor,
            prior, new_acc, hp, bar, bar - hp, discarded)
    end
end

local function on_final_heal(site)
    return function()
        local r = PCSX.getRegisters()
        local seat = tonumber(r.GPR.n.a2) or -1
        local a = (seat >= 0 and seat <= 7) and actor_of(seat) or 0
        local hp  = a ~= 0 and u16(a + 0x14C) or -1
        local bar = a ~= 0 and u16(a + 0x172) or -1
        local acc = a ~= 0 and i32(a + 0x10) or 0
        final_heals[#final_heals + 1] = {
            vsync = g_elapsed, pc = site.bp, who = site.who, seat = seat,
            hp = hp, bar = bar, acc = acc,
        }
        heal_csv:row("%d,0x%08X,%s,%d,%d,%d,%d",
            g_elapsed, site.bp, site.who, seat, hp, bar, acc)
        PCSX.log(string.format(
            "[discard] vsync=%d FINAL HEAL fired: %s seat=%d (hp=%d bar=%d acc=%d)",
            g_elapsed, site.who, seat, hp, bar, acc))
    end
end

local function on_settle_ret()
    local r = PCSX.getRegisters()
    local verdict = tonumber(r.GPR.n.v0) or 0
    local c = u32(CTX_PTR)
    if not in_ram(c) then return end
    local seat = u8(c + 0x13)
    local a = actor_of(seat)
    local t1dd = a ~= 0 and u8(a + 0x1DD) or -1
    if verdict ~= 0 then
        settle_counts.blocked = settle_counts.blocked + 1
    else
        settle_counts.settled = settle_counts.settled + 1
    end
    settle_csv:row("%d,%d,%d,%d,%d",
        g_elapsed, verdict, i16(c + 0x6D8), seat, t1dd)
end

local function arm_bps()
    local c = u32(CTX_PTR)
    if not in_ram(c) then return false end
    for _, w in ipairs(WRITERS) do
        probe.arm_breakpoint(w.bp, "Exec", 4,
            string.format("acc_%08X", w.store), on_acc_write(w))
    end
    for _, s in ipairs(FINAL_HEAL_SITES) do
        probe.arm_breakpoint(s.bp, "Exec", 4,
            string.format("heal_%08X", s.bp), on_final_heal(s))
    end
    probe.arm_breakpoint(SETTLE_RET, "Exec", 4, "settle_ret", on_settle_ret)
    PCSX.log(string.format(
        "[discard] armed %d accumulator writers + %d final-heal sites + settle return on ctx=0x%08X",
        #WRITERS, #FINAL_HEAL_SITES, c))
    return true
end

-- Poke the owned Lost Grail into a party member's accessory slot 7. Pure
-- equipment-id state - the ability bit itself is derived from it by the
-- retail per-frame aggregator FUN_80042558, which is the point.
local function poke_grail(seat)
    local pid = u8(PARTY_ID_TBL + seat)
    if pid < 1 or pid > 4 then
        PCSX.log(string.format("[discard] grail poke: seat %d has bad party id %d",
            seat, pid))
        return
    end
    local rec = CHAR_REC + (pid - 1) * 0x414
    local old = u8(rec + 0x19D)
    probe.write_u8(rec + 0x19D, LOST_GRAIL)
    grail_poked_at = g_elapsed
    PCSX.log(string.format(
        "[discard] vsync=%d poked Lost Grail 0x%02X over accessory 0x%02X " ..
        "(seat %d, record 0x%08X slot +0x19D); no HP/bar/acc field touched",
        g_elapsed, LOST_GRAIL, old, seat, rec))
end

------------------------------------------------------------------
local last_ctx7, ctx7_since = -1, 0

probe.run{
    sstate         = SSTATE,
    capture_frames = FRAMES,
    boot_delay     = 60,
    on_arm         = function() return {} end,
    on_capture     = function(ctx, v)
        g_elapsed = v
        if not armed and v >= 2 then armed = arm_bps() end

        if EQUIP_GRAIL >= 0 and EQUIP_GRAIL <= 2 then
            if not grail_poked_at and v >= 60 then poke_grail(EQUIP_GRAIL) end
            -- Verify the retail aggregator derived the Final Heal gate bit
            -- (record +0xF8 bit 7) from the poked equipment id.
            if grail_poked_at and not grail_bit_seen and v % 30 == 0 then
                local pid = u8(PARTY_ID_TBL + EQUIP_GRAIL)
                local rec = CHAR_REC + (pid - 1) * 0x414
                local w1 = u32(rec + 0xF8)
                if w1 % 0x100 >= 0x80 then
                    grail_bit_seen = v
                    PCSX.log(string.format(
                        "[discard] vsync=%d FUN_80042558 derived ability bit 0x27 " ..
                        "(+0xF8=0x%08X) from the equipped grail", v, w1))
                end
            end
        end

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
        if ctx7 ~= last_ctx7 then
            last_ctx7, ctx7_since = ctx7, 0
        else
            ctx7_since = ctx7_since + 1
            if ctx7 == 0x51 and ctx7_since > 600 and not park_vsync then
                park_vsync = v
                PCSX.log(string.format(
                    "[discard] state 0x51 has been parked %d vsyncs at vsync %d",
                    ctx7_since, v))
            end
        end

        -- A healthy 0x51 dwell measures 83 vsyncs on this save. Once the park
        -- has held ~30x that past detection, the settle.csv tail is already
        -- decisive - stop burning wall clock.
        if park_vsync and v - park_vsync > 2400 then
            ctx.request_quit = true
        end

        -- The party count the all-target arm of FUN_801E7250 scans.
        local party = u8(c + 0x00)
        if party < 1 or party > 3 then party = 3 end

        for seat = 0, party - 1 do
            local a = actor_of(seat)
            if a ~= 0 then
                local hp, bar, acc = u16(a + 0x14C), u16(a + 0x172), i32(a + 0x10)
                local inv = (bar - hp) - acc
                local absorbing = (hp ~= bar) and (acc == 0)

                if absorbing and not absorbing_since[seat] then
                    absorbing_since[seat] = v
                    absorbing_ever[seat] = true
                    note_absorbing(seat, "ENTER", hp, bar, acc)
                elseif (not absorbing) and absorbing_since[seat] then
                    note_absorbing(seat, "LEAVE", hp, bar, acc)
                    absorbing_since[seat] = nil
                end

                -- Log only on change, so the timeline stays readable across a
                -- 4000-vsync capture.
                if inv ~= last_inv[seat] or absorbing then
                    last_inv[seat] = inv
                    local blame = last_acc_writer[seat] or { who = "" }
                    inv_csv:row("%d,0x%02X,%d,%d,%d,%d,%d,%s,%s",
                        v, ctx7, seat, hp, bar, acc, inv,
                        absorbing and "yes" or "no", blame.who)
                end
            end
        end
    end,
    on_done = function()
        local lines = {}
        local function add(f, ...) lines[#lines + 1] = string.format(f, ...) end
        add("=== gaza2 accumulator-discard probe ===")
        add("vsyncs captured: %d", g_elapsed)
        add("")
        add("-- actor+0x10 store hits by site --")
        for _, w in ipairs(WRITERS) do
            add("  0x%08X  %-6s  %-38s  hits=%d",
                w.store, w.kind, w.who, write_counts[w.store] or 0)
        end
        add("")
        add("-- ASSIGN over a non-zero accumulator (the discard) --")
        add("discard events: %d", #discards)
        for _, d in ipairs(discards) do
            add("  vsync=%-6d seat=%d  0x%08X  discarded remainder=%d  " ..
                "(hp=%d bar=%d, so the bar will settle %d away)",
                d.vsync, d.seat, d.pc, d.prior, d.hp, d.bar, d.prior)
        end
        add("")
        add("-- absorbing party slots (hp != bar AND acc == 0) --")
        local any = false
        for seat = 0, 2 do
            if absorbing_ever[seat] then
                any = true
                local still = absorbing_since[seat]
                add("  seat %d: entered the absorbing state%s",
                    seat, still and string.format(" and was STILL in it at vsync %d (since %d)",
                        g_elapsed, still) or " but recovered")
            end
        end
        if not any then add("  none") end
        add("")
        add("-- Lost Grail (Final Heal) instrumentation --")
        if EQUIP_GRAIL >= 0 then
            add("  grail poked onto seat %d at vsync %s; ability bit 0x27 %s",
                EQUIP_GRAIL, tostring(grail_poked_at),
                grail_bit_seen and string.format("derived by vsync %d", grail_bit_seen)
                    or "NEVER derived (poke did not take)")
        else
            add("  no equip poke requested")
        end
        add("final heal firings: %d", #final_heals)
        for _, h in ipairs(final_heals) do
            add("  vsync=%-6d %s seat=%d (hp=%d bar=%d acc-at-call=%d%s)",
                h.vsync, h.who, h.seat, h.hp, h.bar, h.acc,
                h.acc ~= 0 and " <- non-zero: the assign will DISCARD this" or "")
        end
        add("")
        add("-- state 0x51 park --")
        if park_vsync then
            add("  parked from vsync %d (camera yaw 0x%04X)", park_vsync, u16(CAM_YAW))
        else
            add("  no park observed")
        end
        add("settle checks at 0x801E604C: settled=%d not-settled=%d",
            settle_counts.settled, settle_counts.blocked)
        add("")
        if #discards > 0 then
            add("VERDICT: retail reached the bare-assign accumulator store with a")
            add("drain still in flight. The discard is real, not hypothetical.")
        else
            add("VERDICT: no discard observed in this run. Either no restore landed")
            add("mid-drain (check the ASSIGN hit counts above - zero hits means the")
            add("autopilot never used a restore at all, which is a null result, not")
            add("a refutation), or the mechanism does not fire on this path.")
        end
        probe.write_snapshot(probe.out_path("summary.txt"), table.concat(lines, "\n"))
        for _, l in ipairs(lines) do PCSX.log("[discard] " .. l) end
    end,
}
