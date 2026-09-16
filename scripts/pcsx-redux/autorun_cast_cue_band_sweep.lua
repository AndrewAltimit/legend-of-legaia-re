-- autorun_cast_cue_band_sweep.lua
--
-- Does the cast-audio dispatcher `FUN_801F3990`'s character-banded cue band
-- ever fire on a player cast?
--
-- The dispatcher resolves a cue from the acting slot's char-kind byte
-- `DAT_8007BD10[slot]`, the cast class `actor[+0x1E8]` and the queue head
-- `actor[+0x1DF]`, and every arm converges on ONE call site - `jal 0x8004FCC8`
-- at `0x801F3C18` - with `a0` already holding the resolved id. That single
-- instruction is the band's own dispatch site, so an exec breakpoint on it
-- answers the question directly: a hit means the band fired, and `a0` says with
-- which id. `0x801F3A7C` is the dispatcher's other exit, the `+0x1DF == 0xFE`
-- arm that skips the cue dispatcher and starts a CD-XA clip itself.
--
-- The band is character-split (`char_kind * 0x10 + 0xF8..0xFC` on the party
-- leg, `0x20C..0x20E` on the enemy leg), and `FUN_8004FCC8` only takes the
-- CD-XA arm for ids `>= 0x100` - so char-kind 0 resolves below that bound and
-- can fire the band while playing nothing. The probe therefore records BOTH the
-- band hit and whether a clip followed.
--
-- The spread is driven, not resumed into: a plan of `seat:spell` pairs is
-- injected one per window the same way `autorun_w3a_cast_oracle.lua` injects a
-- cast (`actor[+0x1DE] = 2` Magic, `+0x1DF = spell`, `+0x1DD = target seat`),
-- with MP topped up so the action is not refused, while CROSS is tapped to
-- advance the command flow. Several casts per run instead of one.
--
-- Env vars:
--   LEGAIA_SSTATE      battle save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES      capture vsyncs (default 2400)
--   LEGAIA_PLAN        comma list of `seat:spell` (default cycles seats 0..2
--                      over Gimard / Theeder / Vera / Nighto)
--   LEGAIA_PERIOD      vsyncs between injections (default 240)
--   LEGAIA_INJECT_AT   vsync of the first injection (default 30)
--   LEGAIA_TARGET_SEAT value written to `+0x1DD` (default 3)
--   LEGAIA_MP          MP to top the caster up to before each cast (default 999)
--   LEGAIA_OUT_DIR     output directory
--
-- Outputs: cast_cue_band.csv, cast_cue_band.log

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE      = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 2400)
local PLAN_S      = probe.getenv("LEGAIA_PLAN",
    "0:0x81,1:0x81,2:0x81,0:0x82,1:0x83,2:0x85,0:0x84,1:0x86,2:0x88")
local PERIOD      = probe.getenv_num("LEGAIA_PERIOD", 240)
local INJECT_AT   = probe.getenv_num("LEGAIA_INJECT_AT", 30)
local TARGET_SEAT = probe.getenv_num("LEGAIA_TARGET_SEAT", 3)
local MP_TOPUP    = probe.getenv_num("LEGAIA_MP", 999)

local OUT_CSV = probe.out_path("cast_cue_band.csv")
local OUT_LOG = probe.out_path("cast_cue_band.log")

local ACTOR_TABLE  = 0x801C9370
local CTX_PTR      = 0x8007BD24
local CHAR_KIND    = 0x8007BD10
local CAST_CUE     = 0x801F3990     -- dispatcher entry
local BAND_SITE    = 0x801F3C18     -- jal 0x8004FCC8 - every cue arm lands here
local FE_CLIP_SITE = 0x801F3A7C     -- jal 0x8003D53C in the +0x1DF == 0xFE arm
local CUE_DISPATCH = 0x8004FCC8
local CLIP_START   = 0x8003D53C

-- The dispatcher has exactly ONE reference disc-wide: `jal 0x801F3990` at
-- `0x801E3E04`, inside one arm of the battle-action SM in PROT 0898
-- (`find-address-word-refs.py 0x801F3990` reports word=0 jal=1 j=0 branch=0).
-- The arm runs `FUN_801D5854(actor[+2], 6)` and then calls the dispatcher only
-- when `actor[+0x1DA] == actor[+0x1D9]` - the action-queue cursor has reached
-- its end - setting `ctx[7] = 0x3E` on the way in. Both are tapped, so a zero
-- at the dispatcher separates "the arm never ran" from "the arm ran and the
-- queue-cursor guard declined".
local ARM_REACHED  = 0x801E3DE4     -- first instruction after the arm's head call
local ARM_GUARD_OK = 0x801E3DF8     -- sb zero,0x1DA(s3): guard passed

-- Byte fingerprints for the three overlay-resident VAs, so a run cannot rest on
-- an aliased image: `jal 0x8004FCC8`, `jal 0x8003D53C`, `addiu a0,zero,0x20C`.
local FINGERPRINTS = {
    { addr = BAND_SITE,    want = 0x0C013F32, what = "jal 0x8004FCC8" },
    { addr = FE_CLIP_SITE, want = 0x0C00F54F, what = "jal 0x8003D53C" },
    { addr = 0x801F3A1C,   want = 0x2404020C, what = "addiu a0,zero,0x20C" },
    { addr = 0x801E3E04,   want = 0x0C07CE64, what = "jal 0x801F3990" },
}

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[cue_band] " .. s)
end

local function u8(a) return probe.read_u8(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end
local function hex8(v) return string.upper(bit.tohex(n32(v))) end

local function ctxp()
    local c = u32(CTX_PTR)
    if c < 0x80000000 or c >= 0x80200000 then return nil end
    return c
end

local function actor(slot)
    if slot < 0 or slot > 7 then return nil end
    local p = u32(ACTOR_TABLE + slot * 4)
    if p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end

local plan = {}
for tok in string.gmatch(PLAN_S, "[^,%s]+") do
    local seat, spell = string.match(tok, "^(%d+):(.+)$")
    if seat then
        plan[#plan + 1] = { seat = tonumber(seat), spell = tonumber(spell) }
    end
end

local csv
local g_elapsed = 0
local plan_idx = 0
local cur_seat, cur_spell = -1, -1
local n_dispatch, n_band, n_fe, n_cue, n_clip = 0, 0, 0, 0, 0
local n_arm, n_guard = 0, 0
local band_ids = {}
local classes_seen = {}

local function row(ev, a, b, c, note)
    local cx = ctxp()
    local slot = cx and u8(cx + 0x13) or -1
    local ap = actor(slot)
    csv:row("%d,%s,%d,%d,%s,%s,%s,%s,%d,%d,%d,%d,%d,%s",
        g_elapsed, ev, plan_idx, cur_seat,
        cur_spell >= 0 and string.format("0x%02X", cur_spell) or "",
        a or "", b or "", c or "",
        slot, slot >= 0 and u8(CHAR_KIND + slot) or -1,
        ap and u8(ap + 0x1E8) or -1, ap and u8(ap + 0x1DF) or -1,
        ap and u8(ap + 0x1E9) or -1, note or "")
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        csv = probe.csv_open(OUT_CSV,
            "vsync,event,plan_idx,plan_seat,plan_spell,a0,a1,a2," ..
            "act_slot,char_kind,cast_class,queue_head,sub_class,note")
        probe.env.write_manifest("autorun_cast_cue_band_sweep.lua", {
            sstate = SSTATE, frames = FRAMES, plan = PLAN_S,
            period = PERIOD, inject_at = INJECT_AT,
            target_seat = TARGET_SEAT, mp = MP_TOPUP,
        })
        local descs = {}

        local function tap(addr, label, name, cb)
            local d = { addr = addr, hits_ref = { n = 0 }, name = name }
            probe.arm_breakpoint(addr, "Exec", 4, label, function()
                d.hits_ref.n = d.hits_ref.n + 1
                cb()
            end)
            descs[#descs + 1] = d
        end

        tap(ARM_REACHED, "arm_reached", "SM arm at 0x801E3DD8 reached", function()
            n_arm = n_arm + 1
            local ap = actor(0)
            row("sm_arm", "", "", "",
                string.format("seat0 +0x1D9=%d +0x1DA=%d",
                    ap and u8(ap + 0x1D9) or -1, ap and u8(ap + 0x1DA) or -1))
        end)

        tap(ARM_GUARD_OK, "arm_guard", "queue-cursor guard passed", function()
            n_guard = n_guard + 1
            row("sm_arm_guard_ok")
        end)

        tap(CAST_CUE, "cast_cue", "FUN_801F3990 entry", function()
            n_dispatch = n_dispatch + 1
            local cx = ctxp()
            local ap = cx and actor(u8(cx + 0x13)) or nil
            if ap then classes_seen[u8(ap + 0x1E8)] = (classes_seen[u8(ap + 0x1E8)] or 0) + 1 end
            row("dispatch_entry")
        end)

        tap(BAND_SITE, "band_site", "FUN_801F3990 cue band -> jal 0x8004FCC8", function()
            n_band = n_band + 1
            local r = PCSX.getRegisters()
            local id = n32(r.GPR.n.a0)
            band_ids[id] = (band_ids[id] or 0) + 1
            row("band_fire", string.format("0x%04X", id), "", "",
                id >= 0x100 and "xa-eligible" or "below 0x100 (SFX path)")
        end)

        tap(FE_CLIP_SITE, "fe_clip", "FUN_801F3990 0xFE arm -> jal 0x8003D53C", function()
            n_fe = n_fe + 1
            local r = PCSX.getRegisters()
            row("fe_arm", tostring(n32(r.GPR.n.a0)), tostring(n32(r.GPR.n.a1)),
                tostring(n32(r.GPR.n.a2)))
        end)

        tap(CUE_DISPATCH, "cue_dispatch", "FUN_8004FCC8 entry", function()
            n_cue = n_cue + 1
            local r = PCSX.getRegisters()
            local ra = n32(r.GPR.n.ra)
            row("cue_dispatch", string.format("0x%04X", n32(r.GPR.n.a0)), "",
                "0x" .. hex8(ra),
                ra == 0x801F3C1C and "from the band" or "other caller")
        end)

        tap(CLIP_START, "clip_start", "FUN_8003D53C entry", function()
            n_clip = n_clip + 1
            local r = PCSX.getRegisters()
            row("clip_start", tostring(n32(r.GPR.n.a0)), tostring(n32(r.GPR.n.a1)),
                tostring(n32(r.GPR.n.a2)), "0x" .. hex8(n32(r.GPR.n.ra)))
        end)

        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed

        if elapsed == 2 then
            for _, f in ipairs(FINGERPRINTS) do
                local got = u32(f.addr)
                logf("fingerprint [0x%08X] = 0x%s (want 0x%s, %s) %s",
                     f.addr, hex8(got), hex8(f.want), f.what,
                     got == f.want and "OK" or "MISMATCH")
            end
            local cx = ctxp()
            logf("ctx=0x%s act_slot=%d kinds=%d,%d,%d,%d plan=%d entries",
                 hex8(cx or 0), cx and u8(cx + 0x13) or -1,
                 u8(CHAR_KIND), u8(CHAR_KIND + 1), u8(CHAR_KIND + 2),
                 u8(CHAR_KIND + 3), #plan)
        end

        -- Advance the command flow. A released frame between taps is needed or
        -- the menu reads one long press.
        probe.pad_release(probe.BTN.CROSS)
        local sub = elapsed % 20
        if sub >= 10 and sub < 13 then probe.pad_force(probe.BTN.CROSS) end

        if elapsed >= INJECT_AT and #plan > 0 then
            local want = math.floor((elapsed - INJECT_AT) / PERIOD) + 1
            if want <= #plan and want > plan_idx and
                ((elapsed - INJECT_AT) % PERIOD) == 0 then
                local p = plan[want]
                local ap = actor(p.seat)
                if ap then
                    plan_idx = want
                    cur_seat, cur_spell = p.seat, p.spell
                    probe.write_u16(ap + 0x150, MP_TOPUP)
                    probe.write_u8(ap + 0x1DE, 2)
                    probe.write_u8(ap + 0x1DF, p.spell)
                    probe.write_u8(ap + 0x1DD, TARGET_SEAT)
                    logf("inject %d/%d at vsync %d: seat %d spell 0x%02X " ..
                         "(char_kind %d)", want, #plan, elapsed, p.seat, p.spell,
                         u8(CHAR_KIND + p.seat))
                end
            end
        end
    end,

    on_summary = function()
        probe.pad_release(probe.BTN.CROSS)
        logf("injections=%d/%d sm_arm_reached=%d guard_passed=%d " ..
             "dispatcher_entries=%d band_fires=%d fe_arm=%d " ..
             "cue_dispatch=%d clip_start=%d",
             plan_idx, #plan, n_arm, n_guard, n_dispatch, n_band, n_fe,
             n_cue, n_clip)
        local ids = {}
        for id, n in pairs(band_ids) do
            ids[#ids + 1] = string.format("0x%04X x%d", id, n)
        end
        table.sort(ids)
        logf("band cue ids: %s", #ids > 0 and table.concat(ids, " ") or "(none)")
        local cl = {}
        for c, n in pairs(classes_seen) do
            cl[#cl + 1] = string.format("%d x%d", c, n)
        end
        table.sort(cl)
        logf("cast classes (actor+0x1E8) seen at the dispatcher: %s",
             #cl > 0 and table.concat(cl, " ") or "(none)")
        local fh = io.open(OUT_LOG, "w")
        if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
        if csv then csv:close() end
    end,
})
