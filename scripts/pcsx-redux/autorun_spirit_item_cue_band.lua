-- autorun_spirit_item_cue_band.lua
--
-- Drives the ONE caller of the cast audio-cue dispatcher `FUN_801F3990`.
--
-- The dispatcher has a single reference disc-wide: `jal 0x801F3990` at
-- `0x801E3E04`, inside the battle-action SM arm at `0x801E3DD8`. That arm is
-- slot `0x3D` of the SM's `ctx[7]` jump table at `0x801CED44` -
-- `(0x801CEE38 - 0x801CED44) / 4 = 0x3D` - i.e. the Spirit / Item band's
-- "wait for the staged clip" state. The band is entered only from state
-- `0x3C`, and `0x3C` is seeded by exactly two arms of the action-category
-- dispatch (jump table `0x801CF144`, `actor[+0x1DE]`):
--
--   * category 1 (Item), arm `0x801E2E30` - stores `ctx[7] = 0x3C` first and
--     only overrides to `0x28` for item ids `0x98` / `0x99`;
--   * category 2 (Magic), arm `0x801E2EB0` - stores `0x28` first and overrides
--     to `0x3C` only when BOTH the spell's class byte `< 0x14` AND the spell id
--     `< 0x65` (`sltiu v0,a0,0x65` at `0x801E2EF4`).
--
-- So a sweep that injects the player Seru block (`0x81..0x8B`) can never route
-- into the band: every one of those ids fails the `< 0x65` test. This probe
-- drives the band the way the bytes say it is reachable - by hijacking the
-- action CATEGORY at the seed state `0x0C` (arm entry `0x801E2BFC`), before
-- either `lbu +0x1DE` in that arm runs.
--
-- Env vars:
--   LEGAIA_SSTATE       battle save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES       capture vsyncs (default 2400)
--   LEGAIA_PLAN         comma list of `cat:param` injected one per seed arm
--                       (default "1:0x02,1:0x05,1:0x09,1:0x02,1:0x05,1:0x09")
--   LEGAIA_MAX_INJECT   stop injecting after this many seed arms (default 24)
--   LEGAIA_FORCE_E7     when >= 0, also force `actor[+0x1E7]` to this value at
--                       the seed arm, so the clip `0x3C` stages is one the
--                       animation system can converge on (default -1 = leave)
--   LEGAIA_TARGET_SEAT  value written to `+0x1DD` (default 3)
--   LEGAIA_OUT_DIR      output directory
--
-- Outputs: spirit_item_cue_band.csv, spirit_item_cue_band.log

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE      = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 2400)
local PLAN_S      = probe.getenv("LEGAIA_PLAN", "1:0x02,1:0x05,1:0x09,1:0x02,1:0x05,1:0x09")
local MAX_INJECT  = probe.getenv_num("LEGAIA_MAX_INJECT", 24)
local FORCE_E7    = probe.getenv_num("LEGAIA_FORCE_E7", -1)
local TARGET_SEAT = probe.getenv_num("LEGAIA_TARGET_SEAT", 3)

local OUT_CSV = probe.out_path("spirit_item_cue_band.csv")
local OUT_LOG = probe.out_path("spirit_item_cue_band.log")

local ACTOR_TABLE = 0x801C9370
local CTX_PTR     = 0x8007BD24
local CHAR_KIND   = 0x8007BD10

local SEED_ARM    = 0x801E2BFC   -- ctx[7] == 0x0C arm head (injection point)
local ITEM_ARM    = 0x801E2E30   -- category-1 arm: stores ctx[7] = 0x3C
local MAGIC_ARM   = 0x801E2EB0   -- category-2 arm: stores 0x28, may override
local S3C_ARM     = 0x801E3B20   -- ctx[7] == 0x3C arm head
local S3D_ARM     = 0x801E3DE4   -- ctx[7] == 0x3D, after the arm's pose call
local GUARD_OK    = 0x801E3DF8   -- sb zero,0x1DA(s3): +0x1DA == +0x1D9 passed
local CAST_CUE    = 0x801F3990   -- FUN_801F3990 entry
local BAND_SITE   = 0x801F3C18   -- jal 0x8004FCC8 - every cue arm lands here
local CUE_DISPATCH= 0x8004FCC8
local CLIP_START  = 0x8003D53C

-- Byte fingerprints: a run cannot rest on an aliased image.
local FINGERPRINTS = {
    { addr = SEED_ARM,  want = 0x3C02801F, what = "lui v0,0x801f (0x0C arm head)" },
    { addr = ITEM_ARM,  want = 0x3C108008, what = "lui s0,0x8008 (category-1 arm)" },
    { addr = S3C_ARM,   want = 0x92A20002, what = "lbu v0,2(s5) (0x3C arm head)" },
    { addr = 0x801E3B5C,want = 0x2402003D, what = "addiu v0,zero,0x3d (0x3C -> 0x3D)" },
    { addr = S3D_ARM,   want = 0x926301DA, what = "lbu v1,0x1da(s3) (0x3D guard)" },
    { addr = GUARD_OK,  want = 0xA26001DA, what = "sb zero,0x1da(s3) (guard passed)" },
    { addr = 0x801E3E04,want = 0x0C07CE64, what = "jal 0x801F3990" },
}

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[cue_caller] " .. s)
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
    if slot == nil or slot < 0 or slot > 7 then return nil end
    local p = u32(ACTOR_TABLE + slot * 4)
    if p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end

local function active()
    local cx = ctxp()
    if not cx then return nil, -1 end
    local slot = u8(cx + 0x13)
    return actor(slot), slot
end

local plan = {}
for tok in string.gmatch(PLAN_S, "[^,%s]+") do
    local cat, param = string.match(tok, "^(%d+):(.+)$")
    if cat then plan[#plan + 1] = { cat = tonumber(cat), param = tonumber(param) } end
end

local csv
local g_elapsed = 0
local n_seed, n_inject, n_item, n_magic = 0, 0, 0, 0
local n_3c, n_3d, n_guard, n_dispatch, n_band, n_cue, n_clip = 0, 0, 0, 0, 0, 0, 0
local band_ids = {}
local states_seen = {}
local last_state = -1
local cur = { cat = -1, param = -1 }

local function row(ev, a, b, note)
    local ap, slot = active()
    local cx = ctxp()
    csv:row("%d,%s,%d,%s,%s,%s,%s,%d,%d,%d,%d,%d,%d,%d,%s",
        g_elapsed, ev, n_inject,
        cur.cat >= 0 and tostring(cur.cat) or "",
        cur.param >= 0 and string.format("0x%02X", cur.param) or "",
        a or "", b or "",
        slot, slot >= 0 and u8(CHAR_KIND + slot) or -1,
        cx and u8(cx + 7) or -1,
        ap and u8(ap + 0x1DE) or -1, ap and u8(ap + 0x1DF) or -1,
        ap and u8(ap + 0x1D9) or -1, ap and u8(ap + 0x1DA) or -1,
        note or "")
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        csv = probe.csv_open(OUT_CSV,
            "vsync,event,inject_idx,inj_cat,inj_param,a0,ra," ..
            "act_slot,char_kind,ctx7,cat_1de,param_1df,anim_1d9,staged_1da,note")
        probe.env.write_manifest("autorun_spirit_item_cue_band.lua", {
            sstate = SSTATE, frames = FRAMES, plan = PLAN_S,
            max_inject = MAX_INJECT, force_e7 = FORCE_E7,
            target_seat = TARGET_SEAT,
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

        -- The injection: rewrite the action category BEFORE the seed arm reads
        -- it. Both `lbu +0x1DE` sites in the arm are downstream of this head.
        tap(SEED_ARM, "seed_arm", "ctx[7] == 0x0C seed arm", function()
            n_seed = n_seed + 1
            local ap = active()
            if ap and #plan > 0 and n_inject < MAX_INJECT then
                local p = plan[(n_inject % #plan) + 1]
                n_inject = n_inject + 1
                cur.cat, cur.param = p.cat, p.param
                probe.write_u8(ap + 0x1DE, p.cat)
                probe.write_u8(ap + 0x1DF, p.param)
                probe.write_u8(ap + 0x1DD, TARGET_SEAT)
                if FORCE_E7 >= 0 then probe.write_u8(ap + 0x1E7, FORCE_E7) end
                row("seed_inject", "", "",
                    string.format("cat=%d param=0x%02X e7=%d", p.cat, p.param,
                        u8(ap + 0x1E7)))
            else
                row("seed_arm")
            end
        end)

        tap(ITEM_ARM, "item_arm", "category-1 (Item) seed arm", function()
            n_item = n_item + 1
            row("cat1_item")
        end)

        tap(MAGIC_ARM, "magic_arm", "category-2 (Magic) seed arm", function()
            n_magic = n_magic + 1
            row("cat2_magic")
        end)

        tap(S3C_ARM, "state_3c", "ctx[7] == 0x3C pre-arm", function()
            n_3c = n_3c + 1
            local ap = active()
            row("state_3c", "", "",
                string.format("+0x1E7=%d", ap and u8(ap + 0x1E7) or -1))
        end)

        tap(S3D_ARM, "state_3d", "ctx[7] == 0x3D wait (the arm)", function()
            n_3d = n_3d + 1
            if n_3d <= 400 then row("state_3d") end
        end)

        tap(GUARD_OK, "guard_ok", "+0x1DA == +0x1D9 guard passed", function()
            n_guard = n_guard + 1
            row("guard_ok")
        end)

        tap(CAST_CUE, "cast_cue", "FUN_801F3990 entry", function()
            n_dispatch = n_dispatch + 1
            local r = PCSX.getRegisters()
            row("dispatch_entry", "", "0x" .. hex8(n32(r.GPR.n.ra)))
        end)

        tap(BAND_SITE, "band_site", "cue band -> jal 0x8004FCC8", function()
            n_band = n_band + 1
            local r = PCSX.getRegisters()
            local id = n32(r.GPR.n.a0)
            band_ids[id] = (band_ids[id] or 0) + 1
            row("band_fire", string.format("0x%04X", id), "",
                id >= 0x100 and "xa-eligible" or "below 0x100 (SFX path)")
        end)

        tap(CUE_DISPATCH, "cue_dispatch", "FUN_8004FCC8 entry", function()
            n_cue = n_cue + 1
            local r = PCSX.getRegisters()
            local ra = n32(r.GPR.n.ra)
            if ra == 0x801F3C1C then
                row("cue_dispatch", string.format("0x%04X", n32(r.GPR.n.a0)),
                    "0x" .. hex8(ra), "from the band")
            end
        end)

        tap(CLIP_START, "clip_start", "FUN_8003D53C entry", function()
            n_clip = n_clip + 1
            local r = PCSX.getRegisters()
            if n_clip <= 200 then
                row("clip_start", tostring(n32(r.GPR.n.a0)),
                    "0x" .. hex8(n32(r.GPR.n.ra)))
            end
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
            logf("ctx=0x%s act_slot=%d plan=%d entries max_inject=%d force_e7=%d",
                 hex8(cx or 0), cx and u8(cx + 0x13) or -1, #plan, MAX_INJECT,
                 FORCE_E7)
        end

        -- Advance the command flow. A released frame between taps is needed or
        -- the menu reads one long press.
        probe.pad_release(probe.BTN.CROSS)
        local sub = elapsed % 20
        if sub >= 10 and sub < 13 then probe.pad_force(probe.BTN.CROSS) end

        local cx = ctxp()
        local st = cx and u8(cx + 7) or -1
        if st ~= last_state then
            states_seen[st] = (states_seen[st] or 0) + 1
            row("ctx7", "", "", string.format("from 0x%02X", last_state))
            last_state = st
        end
    end,

    on_summary = function()
        probe.pad_release(probe.BTN.CROSS)
        logf("seed_arms=%d injections=%d cat1_item=%d cat2_magic=%d " ..
             "state_3c=%d state_3d_frames=%d guard_passed=%d " ..
             "dispatcher=%d band_fires=%d cue_from_band=%d clip_start=%d",
             n_seed, n_inject, n_item, n_magic, n_3c, n_3d, n_guard,
             n_dispatch, n_band, n_cue, n_clip)
        local ids = {}
        for id, n in pairs(band_ids) do
            ids[#ids + 1] = string.format("0x%04X x%d", id, n)
        end
        table.sort(ids)
        logf("band cue ids: %s", #ids > 0 and table.concat(ids, " ") or "(none)")
        local st = {}
        for s, n in pairs(states_seen) do
            st[#st + 1] = string.format("0x%02X x%d", s, n)
        end
        table.sort(st)
        logf("ctx[7] states entered: %s", table.concat(st, " "))
        local fh = io.open(OUT_LOG, "w")
        if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
        if csv then csv:close() end
    end,
})
