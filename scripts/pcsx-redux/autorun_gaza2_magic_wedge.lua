-- autorun_gaza2_magic_wedge.lua
--
-- Gaza 2 magic-softlock hunt, run against a save state parked at the START
-- of the Sim-Seru Gaza fight (monster id 166, scene korb3, ctx+7 == 0).
--
-- The community symptom is an endless battle-camera orbit after a player
-- magic cast. The orbit itself is the unconditional idle azimuth sweep
-- (FUN_801D0748 stepping _DAT_8007B792), so it is a SYMPTOM: the real stall
-- is the battle-action SM (FUN_801E295C, cursor ctx+7) parking in the
-- summon band 0x32..0x38 and never reaching 0x50 (done).
--
-- Per docs/subsystems/battle-action.md, the cast-effect census FUN_801E09F8
-- RECOMPUTES both exit counters from scratch every frame:
--
--   ctx+0x249  actors still mid-animation: +1 per live actor with +0x1D9 != 0,
--              less party actors whose +0x1D9 == 8
--   ctx+0x24D  active spell-children over the ctx+0x252.. slot array
--
-- They are live counts, not latched flags. So a band that never exits means
-- some ACTOR never clears +0x1D9 (pins +0x249) or some spell CHILD never
-- retires (pins +0x24D). This probe is built to tell those two apart and to
-- name the owning slot, which is exactly what the timeline records.
--
-- Outputs (under captures/gaza2_magic_wedge/<ts>/):
--   timeline.csv        change-triggered per-vsync SM + signal timeline
--   signal_writes.csv   write attribution (pc/ra) for ctx+7 and the counters
--   stall.txt           full diagnostic dump the first time the cursor parks
--   summary.txt         terse run verdict (band reached, stalled y/n)
--
-- Knobs (env):
--   LEGAIA_PAD_SCRIPT   ";"-separated "vsync:BUTTON[:hold]" pad program, e.g.
--                       "200:CROSS;260:DOWN;300:CROSS". Button names are the
--                       probe.pad BTN keys. Default: empty (passive observe).
--   LEGAIA_STALL_N      vsyncs of an unchanged ctx+7 that counts as a stall
--                       (default 900; a healthy summon 0x36 runs ~760).
--   LEGAIA_FRAMES       capture length in vsyncs.
--
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_gaza2_magic_wedge.lua \
--     --sstate ~/.config/pcsx-redux/SCUS94254.sstate9 --frames 3000

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local watch = require("probe.watch")
local pad   = require("probe.pad")

local SSTATE  = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES  = probe.getenv_num("LEGAIA_FRAMES", 3000)
local STALL_N = probe.getenv_num("LEGAIA_STALL_N", 900)
local SCRIPT  = probe.getenv("LEGAIA_PAD_SCRIPT", "")
-- LEGAIA_UI_TRACE=1 arms FUN_801D8DE8 (the battle UI-element scheduler) and
-- logs every (effect_id, mode) call. Used to discover which pad button opens
-- the magic submenu: the arts phase takes the FACE buttons as attack
-- DIRECTIONS (Triangle=Up, Cross=Down, Square=Left, Circle=Right; see
-- docs/subsystems/arts-command-gauge.md), so the menu opener is elsewhere and
-- a UI-element burst is the cheapest way to see a menu appear at all.
local UI_TRACE = probe.getenv_num("LEGAIA_UI_TRACE", 0)
local UI_ELEMENT_FN = 0x801D8DE8
-- LEGAIA_SHOT_EVERY=N writes a framebuffer grab every N vsyncs (decode with
-- scripts/pcsx-redux/decode_pcsx_screen.py). Orientation only - a screenshot
-- lags the draw and is never used here as a state read.
local SHOT_EVERY = probe.getenv_num("LEGAIA_SHOT_EVERY", 0)
-- LEGAIA_AUTOPILOT=N drives the battle indefinitely: every N vsyncs it presses
-- the next button of LEGAIA_AUTOPILOT_SEQ, cycling forever. This is the
-- sampling instrument for a rare RNG-gated wedge. A fixed pad TIMELINE cannot
-- sample it: resuming a fixed save state with a fixed script is deterministic,
-- and a +/-1 vsync jitter is absorbed by the menu's edge detection (measured:
-- jittered replays reproduce the same 1224-vsync summon dwell exactly). The
-- autopilot instead keeps the fight going for many rounds, so the enemy acts,
-- damage lands, effects overlap and the action mix actually varies.
--
-- The macro MUST contain DOWN presses. A cycle of only CROSS/RIGHT always
-- confirms whatever the spell cursor already sits on, which is the FIRST
-- entry of the acting character's magic list - one fixed healing spell, cast
-- every single turn. That is not a sample, and it is measurably the weakest
-- possible probe of this bug: across every such capture `ctx+0x24D` stays 0
-- for the whole cast, i.e. the heal spawns NO spell children at all, so the
-- +0x24D census, the in-flight/impact stepper and the damage kernel are never
-- exercised. DOWN walks the spell list so offensive casts (which do populate
-- ctx[0x252..0x255]) actually get reached.
local AUTOPILOT = probe.getenv_num("LEGAIA_AUTOPILOT", 0)
local AUTOPILOT_SEQ = probe.getenv("LEGAIA_AUTOPILOT_SEQ",
    "CROSS,RIGHT,CROSS,DOWN,CROSS,CROSS,RIGHT,CROSS,DOWN,DOWN,CROSS,CROSS,CROSS,CROSS")

if probe.getenv("LEGAIA_CORE", "") == "dynarec" then
    PCSX.log("[gaza2] REFUSING --fast launch: Lua breakpoints never fire under the recompiler")
    PCSX.quit(3)
    return
end

local CTX_PTR = 0x8007BD24
local ACTORS  = 0x801C9370
local CAM_YAW = 0x8007B792

local SLOT_BIT = { [0] = 1, 2, 4, 8, 16, 32, 64, 128 }

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end

------------------------------------------------------------------
-- Pad program: parse "vsync:BUTTON[:hold]" entries into a schedule.
local pad_events = {}
for item in SCRIPT:gmatch("[^;]+") do
    local at, name, hold = item:match("^%s*(%d+):(%a+):?(%d*)%s*$")
    if at then
        local btn = pad.BTN[name:upper()]
        if btn then
            pad_events[#pad_events + 1] = {
                at = tonumber(at), btn = btn,
                hold = tonumber(hold) or 4, name = name:upper(),
            }
        else
            PCSX.log("[gaza2] unknown pad button in LEGAIA_PAD_SCRIPT: " .. name)
        end
    elseif item:match("%S") then
        PCSX.log("[gaza2] unparsable pad script item: " .. item)
    end
end
table.sort(pad_events, function(a, b) return a.at < b.at end)
PCSX.log(string.format("[gaza2] pad program: %d event(s)", #pad_events))

local auto_seq = {}
if AUTOPILOT > 0 then
    for name in AUTOPILOT_SEQ:gmatch("[^,]+") do
        local btn = pad.BTN[name:upper():gsub("%s", "")]
        if btn then
            auto_seq[#auto_seq + 1] = { btn = btn, name = name:upper() }
        else
            PCSX.log("[gaza2] unknown autopilot button: " .. name)
        end
    end
    PCSX.log(string.format("[gaza2] autopilot: every %d vsyncs, cycle of %d",
        AUTOPILOT, #auto_seq))
end
local auto_i = 1

local g_elapsed = 0
local hits_csv = probe.csv_open(probe.out_path("signal_writes.csv"),
    "tick,label,addr,pc,ra,value")
local w = watch.new{
    csv         = hits_csv,
    detail_path = probe.out_path("signal_writes.detail.txt"),
    max_detail  = 32,
    elapsed     = function() return g_elapsed end,
}

-- Per-actor +0x1D9 is the whole point: ctx+0x249 is a census over these.
-- `p4mask` is bit s set when actor[s]+0x4 != 0. That field is the census's
-- own per-slot gate (disassembly at 0x801E0A60/0x801E0A68: `lw v0,0x4(v1)` /
-- `beq v0,zero,<skip slot>`), so a slot only contributes to ctx+0x249 when
-- +0x4 is non-zero. The summon band clears +0x4 across the table at state
-- 0x34 and restores it at 0x36, which is exactly the window where a
-- never-ending actor animation could get re-admitted to the census.
-- `stream` is actor[+0x1DF..+0x1E6], the per-action parameter byte stream the
-- command phase appends to as the player queues arts commands (and where a
-- committed Magic cast leaves its spell id - the player Seru-magic block is
-- 0x81..=0x8B). `cur` is the stream cursor actor[+0x15].
local timeline = probe.csv_open(probe.out_path("timeline.csv"),
    "vsync,ctx7,seat,cat,move_id,c249,c24a,c24b,c24c,c24d,c24e," ..
    "a1fa,a1d9,a1da,a21b,cur,cam_yaw," ..
    "d9_0,d9_1,d9_2,d9_3,d9_4,d9_5,d9_6,d9_7," ..
    "hp_0,hp_1,hp_2,hp_3,p4mask,child0,child1,child2,child3,stream")

local ui_csv = probe.csv_open(probe.out_path("ui_elements.csv"),
    "vsync,effect_id,mode,ra")

local armed = false
local watched_actor = 0
local last_key = ""
local last_ctx7, ctx7_since = -1, 0
local stall_dumped = false
local stall_confirmed = false
local band_seen = {}
local band_max_dwell = {}

-- The cast bands: magic/item 0x28..0x2E and summon 0x32..0x38. A stall only
-- counts inside these - every other long park (0x00 waiting for the player,
-- 0x20 attack-return) is ordinary.
local function in_cast_band(st)
    return (st >= 0x28 and st <= 0x2E) or (st >= 0x32 and st <= 0x38)
end
local pad_idx, pad_release_at, pad_btn_held = 1, nil, nil

local function take_shot(tag)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not (ok and ss ~= nil and ss.data ~= nil) then
        PCSX.log("[gaza2] WARN: takeScreenShot failed: " .. tostring(ss))
        return
    end
    local base = probe.out_path(string.format("shot_%s.screen", tag))
    local fh = io.open(base, "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data)); fh:close()
    local mf = io.open(base .. ".meta", "w")
    if mf ~= nil then
        mf:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            tonumber(ss.width), tonumber(ss.height),
            (ss.bpp == 0) and 16 or 24))
        mf:close()
    end
end

local function actor_of(seat)
    local a = u32(ACTORS + seat * 4)
    return in_ram(a) and a or 0
end

local function arm_watches()
    local c = u32(CTX_PTR)
    if not in_ram(c) then return false end
    local seat = u8(c + 0x13)
    local actor = actor_of(seat)
    w:arm(c + 7,     1, "ctx7_cursor")
    w:arm(c + 0x249, 1, "ctx_249")
    w:arm(c + 0x24C, 1, "ctx_24c")
    w:arm(c + 0x24D, 1, "ctx_24d")
    if actor ~= 0 then
        watched_actor = actor
        w:arm(actor + 0x1FA, 1, "actor_1fa")
        w:arm(actor + 0x1D9, 1, "actor_1d9")
        w:arm(actor + 0x21B, 1, "actor_21b")
    end
    if UI_TRACE ~= 0 then
        probe.arm_breakpoint(UI_ELEMENT_FN, "Exec", 4, "ui_element", function()
            local r = PCSX.getRegisters()
            local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x100
            local a1 = (tonumber(r.GPR.n.a1) or 0) % 0x100
            local ra = tonumber(r.GPR.n.ra) or 0
            ui_csv:row("%d,0x%02X,%d,0x%08X", g_elapsed, a0, a1, ra)
        end)
    end
    PCSX.log(string.format(
        "[gaza2] armed: ctx=0x%08X seat=%d actor=0x%08X", c, seat, actor))
    return true
end

-- Full diagnostic dump: everything needed to say WHICH count is stuck and
-- WHO owns it, without a second run.
local function dump_stall(c, why, fname)
    local lines = {}
    local function add(f, ...) lines[#lines + 1] = string.format(f, ...) end
    add("=== gaza2 stall dump: %s ===", why)
    add("vsync=%d  ctx=0x%08X  ctx+7=0x%02X (unchanged for %d vsyncs)",
        g_elapsed, c, u8(c + 7), ctx7_since)
    add("acting seat ctx+0x13 = %d", u8(c + 0x13))
    add("")
    add("-- exit counters (recomputed each frame by FUN_801E09F8) --")
    add("ctx+0x249 (actor anim census) = %d   <- gates magic exit 0x2E", u8(c + 0x249))
    add("ctx+0x24A (party sole target) = %d", u8(c + 0x24A))
    add("ctx+0x24B (mons  sole target) = %d", u8(c + 0x24B))
    add("ctx+0x24C (hit counter)       = %d", u8(c + 0x24C))
    add("ctx+0x24D (spell children)    = %d", u8(c + 0x24D))
    add("ctx+0x24E (flight phase)      = %d", u8(c + 0x24E))
    add("ctx+0x6D8 (band timer, u16)   = %d", u16(c + 0x6D8))
    add("ctx+0x276/0x277/0x278        = %d / %d / %d",
        u8(c + 0x276), u8(c + 0x277), u8(c + 0x278))
    add("")
    add("-- the ctx+0x24D census array: ctx[0x252..0x255], one byte per")
    add("   in-flight spell child; +0x24D = how many are non-zero --")
    local row = {}
    for i = 0, 3 do row[#row + 1] = string.format("[%d]=%02X", i, u8(c + 0x252 + i)) end
    add("  %s", table.concat(row, "  "))
    add("-- ctx[0x24E..0x251] per-child phase (the census only recomputes")
    add("   +0x24D when at least one of these is non-zero) --")
    row = {}
    for i = 0, 3 do row[#row + 1] = string.format("[%d]=%02X", i, u8(c + 0x24E + i)) end
    add("  %s", table.concat(row, "  "))
    add("-- ctx+0x6C6.. per-slot timers --")
    row = {}
    for i = 0, 15 do row[#row + 1] = string.format("%02X", u8(c + 0x6C6 + i)) end
    add("  %s", table.concat(row, " "))
    add("")
    add("-- actor table. The ctx+0x249 census gate is +0x4 != 0 (NOT hp):")
    add("   contributes +1 when +0x4 != 0 and +0x1D9 != 0, then -1 again for")
    add("   a PARTY slot (<3) whose +0x1D9 == 8 --")
    add("  seat  actor       +7   +0x4        hp     +0x1D9 +0x1DA +0x1DC" ..
        " +0x1DE +0x1DF +0x1F5 +0x1FA +0x21B +0x21C +0x16E")
    for seat = 0, 7 do
        local a = actor_of(seat)
        if a ~= 0 then
            add("  %4d  0x%08X  0x%02X 0x%08X %5d   0x%02X   0x%02X   0x%02X" ..
                "   0x%02X   0x%02X   0x%02X   %4d   %4d   0x%02X  0x%04X",
                seat, a, u8(a + 0x07), u32(a + 0x4), u16(a + 0x14C),
                u8(a + 0x1D9), u8(a + 0x1DA), u8(a + 0x1DC), u8(a + 0x1DE),
                u8(a + 0x1DF), u8(a + 0x1F5), u8(a + 0x1FA), u8(a + 0x21B),
                u8(a + 0x21C), u16(a + 0x16E))
        end
    end
    add("")
    add("-- verdict --")
    local c249, c24d = u8(c + 0x249), u8(c + 0x24D)
    add("ctx+0x249=%d ctx+0x24D=%d", c249, c24d)
    -- Reproduce the census exactly as the disassembly computes it, so the
    -- dump names the owning slot rather than guessing.
    local pinners = {}
    for seat = 0, 7 do
        local a = actor_of(seat)
        if a ~= 0 and u32(a + 0x4) ~= 0 and u8(a + 0x1D9) ~= 0 then
            if not (seat < 3 and u8(a + 0x1D9) == 8) then
                pinners[#pinners + 1] = string.format(
                    "seat %d (+0x4=0x%08X, +0x1D9=0x%02X)",
                    seat, u32(a + 0x4), u8(a + 0x1D9))
            end
        end
    end
    local kids = {}
    for i = 0, 3 do
        if u8(c + 0x252 + i) ~= 0 then
            kids[#kids + 1] = string.format("child %d (target=0x%02X, phase=0x%02X)",
                i, u8(c + 0x252 + i), u8(c + 0x24E + i))
        end
    end
    if #pinners > 0 then
        add("ACTOR-ANIM pin (+0x249): %s", table.concat(pinners, ", "))
    else
        add("no actor pins +0x249 - the stall is NOT the actor-anim census")
    end
    if #kids > 0 then
        add("SPELL-CHILD pin (+0x24D): %s", table.concat(kids, ", "))
    else
        add("no live spell child pins +0x24D - the stall is NOT the child census")
    end
    if #pinners == 0 and #kids == 0 then
        add("NEITHER census is pinned: whatever this state waits on, it is not")
        add("+0x249 / +0x24D - re-read the state body before blaming the counters")
    end
    probe.write_snapshot(probe.out_path(fname or "stall.txt"),
        table.concat(lines, "\n"))
    for _, l in ipairs(lines) do PCSX.log("[gaza2] " .. l) end
end

probe.run{
    sstate         = SSTATE,
    capture_frames = FRAMES,
    boot_delay     = 60,
    on_arm         = function() return { "deferred" } end,
    on_capture     = function(ctx, v)
        g_elapsed = v
        if not armed and v >= 2 then armed = arm_watches() end

        -- Pad program.
        if pad_release_at and v >= pad_release_at then
            pad.release(pad_btn_held)
            pad_release_at, pad_btn_held = nil, nil
        end
        while pad_idx <= #pad_events and pad_events[pad_idx].at <= v do
            local e = pad_events[pad_idx]
            if pad_btn_held then pad.release(pad_btn_held) end
            pad.force(e.btn)
            pad_btn_held, pad_release_at = e.btn, v + e.hold
            PCSX.log(string.format("[gaza2] pad %s at vsync %d", e.name, v))
            pad_idx = pad_idx + 1
        end
        if AUTOPILOT > 0 and #auto_seq > 0 and v % AUTOPILOT == 0 then
            local e = auto_seq[auto_i]
            auto_i = (auto_i % #auto_seq) + 1
            if pad_btn_held then pad.release(pad_btn_held) end
            pad.force(e.btn)
            pad_btn_held, pad_release_at = e.btn, v + 4
        end

        if SHOT_EVERY > 0 and v % SHOT_EVERY == 0 then
            take_shot(string.format("%05d", v))
        end

        local c = u32(CTX_PTR)
        if not in_ram(c) then return end

        local ctx7 = u8(c + 7)
        if ctx7 ~= last_ctx7 then
            if last_ctx7 >= 0 and ctx7_since > (band_max_dwell[last_ctx7] or -1) then
                band_max_dwell[last_ctx7] = ctx7_since
            end
            last_ctx7, ctx7_since = ctx7, 0
            band_seen[ctx7] = (band_seen[ctx7] or 0) + 1
        else
            ctx7_since = ctx7_since + 1
            if ctx7_since > (band_max_dwell[ctx7] or -1) then
                band_max_dwell[ctx7] = ctx7_since
            end
        end

        local seat = u8(c + 0x13)
        local actor = actor_of(seat)
        local d9, hp, p4mask = {}, {}, 0
        for s = 0, 7 do
            local a = actor_of(s)
            d9[#d9 + 1] = a ~= 0 and u8(a + 0x1D9) or 255
            -- No `<<`: the PCSX-Redux Lua sandbox is LuaJIT (5.1 syntax), where
            -- the bitwise operators do not exist.
            if a ~= 0 and u32(a + 0x4) ~= 0 then
                p4mask = p4mask + SLOT_BIT[s]
            end
            if s < 4 then hp[#hp + 1] = a ~= 0 and u16(a + 0x14C) or 0 end
        end
        -- ctx[0x252..0x255]: the FOUR spell-child target slots ctx+0x24D counts
        -- (disassembly 0x801E0BB4..0x801E0BEC: s3 = 0..3, `lbu v0,0x252(v0)`,
        -- `++ctx[0x24d]` per non-zero byte).
        local child = {}
        for i = 0, 3 do child[#child + 1] = u8(c + 0x252 + i) end

        local stream = {}
        for i = 0, 7 do
            stream[#stream + 1] = string.format("%02X",
                actor ~= 0 and u8(actor + 0x1DF + i) or 0)
        end
        local stream_s = table.concat(stream, " ")

        local key = string.format(
            "0x%02X,%d,0x%02X,0x%02X,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
            ctx7, seat,
            actor ~= 0 and u8(actor + 0x1DE) or 255,
            actor ~= 0 and u8(actor + 0x1DF) or 255,
            u8(c + 0x249), u8(c + 0x24A), u8(c + 0x24B),
            u8(c + 0x24C), u8(c + 0x24D), u8(c + 0x24E),
            actor ~= 0 and u8(actor + 0x1FA) or 255,
            actor ~= 0 and u8(actor + 0x1D9) or 255,
            actor ~= 0 and u8(actor + 0x1DA) or 255,
            actor ~= 0 and u8(actor + 0x21B) or 255,
            actor ~= 0 and u8(actor + 0x15) or 255)
        local full = key .. "|" .. table.concat(d9, ",") .. "|" .. stream_s
            .. "|" .. p4mask .. "|" .. table.concat(child, ",")
        if full ~= last_key then
            last_key = full
            timeline:row("%d,%s,%d,%s,%s,0x%02X,%s,%s", v, key, u16(CAM_YAW),
                table.concat(d9, ","), table.concat(hp, ","), p4mask,
                table.concat(child, ","), stream_s)
        end

        -- Stall detection. Only inside the cast bands: on this save a HEALTHY
        -- summon 0x36 measures ~1224 vsyncs, so STALL_N must sit above that
        -- (the ~760 figure from the gimard baseline is a different fight and
        -- is not the threshold to use here).
        if not stall_dumped and ctx7_since >= STALL_N and in_cast_band(ctx7) then
            stall_dumped = true
            dump_stall(c, string.format("ctx+7=0x%02X parked >= %d vsyncs", ctx7, STALL_N))
            if SHOT_EVERY == 0 then take_shot("stall") end
        end
        -- Confirm the wedge is permanent rather than merely slow, then stop:
        -- a second dump at 2x the threshold, still in the same state, is the
        -- difference between "long" and "never exits".
        if stall_dumped and not stall_confirmed
                and ctx7_since >= STALL_N * 2 and in_cast_band(ctx7) then
            stall_confirmed = true
            dump_stall(c, string.format(
                "CONFIRMED: ctx+7=0x%02X still parked at %d vsyncs", ctx7, ctx7_since),
                "stall_confirmed.txt")
            ctx.request_quit = true
        end
    end,
    on_summary     = function()
        local c = u32(CTX_PTR)
        local lines = {}
        local function add(f, ...) lines[#lines + 1] = string.format(f, ...) end
        add("gaza2 magic-wedge run summary")
        add("capture vsyncs: %d", g_elapsed)
        add("signal writes:  %d", w:total())
        add("stall dumped:   %s", tostring(stall_dumped))
        add("stall confirmed: %s", tostring(stall_confirmed))
        if in_ram(c) then
            add("final ctx+7 = 0x%02X (unchanged %d vsyncs), " ..
                "ctx+0x249=%d ctx+0x24D=%d",
                u8(c + 7), ctx7_since, u8(c + 0x249), u8(c + 0x24D))
        end
        local seen = {}
        for st in pairs(band_seen) do seen[#seen + 1] = st end
        table.sort(seen)
        local parts = {}
        for _, st in ipairs(seen) do
            parts[#parts + 1] = string.format("0x%02X x%d", st, band_seen[st])
        end
        add("ctx+7 states visited: %s", table.concat(parts, ", "))
        -- Per-state max dwell makes every attempt a measurement even when the
        -- wedge does not fire: the sweep compares the 0x36 dwell distribution.
        local dparts = {}
        for _, st in ipairs(seen) do
            dparts[#dparts + 1] = string.format("0x%02X=%d", st,
                band_max_dwell[st] or 0)
        end
        add("ctx+7 max dwell: %s", table.concat(dparts, ", "))
        add("cast-band max dwell: %d", (function()
            local m = 0
            for st, d in pairs(band_max_dwell) do
                if in_cast_band(st) and d > m then m = d end
            end
            return m
        end)())
        local text = table.concat(lines, "\n")
        probe.write_snapshot(probe.out_path("summary.txt"), text)
        for _, l in ipairs(lines) do PCSX.log("[gaza2] " .. l) end
        hits_csv:close()
        timeline:close()
        ui_csv:close()
    end,
}
