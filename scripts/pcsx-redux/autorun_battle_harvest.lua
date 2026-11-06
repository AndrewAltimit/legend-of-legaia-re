-- autorun_battle_harvest.lua
--
-- Closed-loop retail-capture harvester: from a field (or battle) save state,
-- walk until a random encounter starts, then play every round with Spirit
-- for each member so the monsters act, and write a raw save state each time
-- a monster's action is on screen. The states become retail-compare corpus
-- candidates for the uncovered enemy attacks and monsters
-- (docs/tooling/capture-coverage.md); classify them with
-- `legaia-engine retail-compare --census` before registering any.
--
-- Pure retail play unless a LEGAIA_WANT* knob below is set: no RAM writes, no
-- pokes, the pad is the only input.
--
-- Field: hold a direction, cycling Up / Right / Down / Left every
-- LEGAIA_WALK_LEG vsyncs, with a Cross every so often to clear a dialogue
-- box. Battle (game mode 0x14 / 0x15, ctx = *0x8007BD24):
--   ctx[+0x06] == 0x1E  round prompt  -> Cross (Begin)
--   ctx[+0x06] == 0x28  command ring  -> Down (Spirit) for the first
--                       LEGAIA_SPIRIT_ROUNDS rounds, then Left (Attack) so
--                       the fight ends and the walk finds the next one
--   ctx[+0x06] == 0x6E  commit check  -> Cross (Begin)
--   other menu states (results, banners) -> Cross every 20 vsyncs
--   ctx[+0x06] == 0xFF  the action SM runs: no input
-- A capture is taken when the acting seat ctx[+0x13] is a monster seat (>= 3)
-- and the action-state cursor ctx[+0x07] has sat LEGAIA_HOLD vsyncs in a
-- mid-action state (strike loop 0x1E, cast 0x2B / 0x2C, module 0x6F / 0x70);
-- at most LEGAIA_PER_KEY captures per (monster id, category, queued id), the
-- queued id dropped for a physical attack (its queue bytes are swings).
--
-- Env vars:
--   LEGAIA_SSTATE    source save state (must be a retail-disc state)
--   LEGAIA_OUT_DIR   output dir: <name>.rawsstate per capture + harvest.log
--   LEGAIA_FRAMES    vsyncs to run (default 12000)
--   LEGAIA_WALK_LEG  vsyncs per walk direction (default 50)
--   LEGAIA_HOLD      vsyncs inside a mid-action state before the capture (default 8)
--   LEGAIA_PER_KEY   captures per action key (default 1)
--   LEGAIA_MAX_CAPS  stop after this many captures (default 12)
--   LEGAIA_PREFIX    capture name prefix (default "harvest")
--   LEGAIA_SPIRIT_ROUNDS  rounds of Spirit before attacking (default 3)
--   LEGAIA_WALK_SEED nonzero = pick each leg's direction (the four plus the
--                    diagonals) and length (30..150 vsyncs) off a seeded
--                    LCG instead of the fixed Up/Right/Down/Left cycle,
--                    so a long walk wanders far enough to find doors
--   LEGAIA_SCENES    1 = also save one field state per scene the walk reaches:
--                    once the loaded scene (label 0x8007050C and define
--                    0x80084540 both stable) has run LEGAIA_SCENE_SETTLE
--                    field vsyncs (default 150) and the player has moved
--                    since the scene entered (the walk has control); named
--                    <prefix>_scene_<label>_v<vsync>. The start scene counts.
--   LEGAIA_WANT      `;`-separated formations to fight in order, each `+`-joined
--                    monster ids in decimal (`147+18;15+120`). In the field the
--                    probe finds the scene's formation row holding exactly those
--                    ids, points every region of the live MAN's region table at
--                    that row alone (+6 base, +7 count 1), gives it a rate and
--                    runs the step counter 0x8007B5FC down, so the next step
--                    rolls it through retail's own FUN_801D9E1C: the stage id,
--                    the world-map return point and the battle flags all stay
--                    the scene's. The writes stay resident in the saved states
--                    (the MAN's region rows and nothing else). The run ends after
--                    the last formation. A formation no row holds is skipped
--                    unless LEGAIA_WANT_FORCE=1, which writes it over row 0
--                    (a synthetic fight: say so in the state's description).
--   LEGAIA_WANT_WIDE 1 = also widen every region's tile box to the whole map,
--                    so the roll fires wherever the walk stands: a scene whose
--                    live condition group covers only tiles the walk never
--                    reaches (concnow) rolls nothing otherwise. Say so in the
--                    state's description.
--   LEGAIA_TOUGH     1 = at each round prompt write every party seat's HP, its
--                    readout and its pending delta back to full, so a low-level
--                    party outlasts the rounds; 2 = also raise each seat's
--                    maximum to 9999 first, for a party a single hit would
--                    wipe (the raised HP stays in the saved states and on the
--                    HUD: say so in the state's description)
--   LEGAIA_WOUND     round number at whose prompt every monster's HP drops to a
--                    third of its maximum: the heal arms need a patient below
--                    half HP (0 = never)
--   LEGAIA_WOUND_EVERY  1 = repeat that cut at every later round prompt: a heal
--                    lifts its patient back above half HP, and the arm then has
--                    nobody to heal
--   LEGAIA_FINISH    1 = once the Spirit rounds are over, write every monster's
--                    HP to 1 at the prompt, so the party's attacks end the fight
--   LEGAIA_HOLD_MODULE / LEGAIA_HOLD_MAGIC  vsyncs into a module / cast band
--                    before its capture (default LEGAIA_HOLD)
--   LEGAIA_SAVE_PRE  1 = also keep a pre-cast state for each cast captured: the
--                    whole emulator state at the action's state 0x0A
--                    (<key>_pre_...), before the pick, which replays the same
--                    cast on its own for the cast-sequence oracle
--   LEGAIA_AUDIO_LOG 1 = write audio.csv, one row per vsync: the game mode, the
--                    track word, the field-BGM and battle sound sources (open,
--                    libsnd sequence, its play flags, tick wait, read cursor)
--                    and the sounding voice records split by owner: what a
--                    fight does to the music between the step and the return
--
-- No breakpoints are armed, so --fast (dynarec) runs are fine.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 12000)
local WALK_LEG = probe.getenv_num("LEGAIA_WALK_LEG", 50)
local HOLD     = probe.getenv_num("LEGAIA_HOLD", 8)
local PER_KEY  = probe.getenv_num("LEGAIA_PER_KEY", 1)
local MAX_CAPS = probe.getenv_num("LEGAIA_MAX_CAPS", 12)
local PREFIX   = probe.getenv("LEGAIA_PREFIX", "harvest")
local SPIRIT_ROUNDS = probe.getenv_num("LEGAIA_SPIRIT_ROUNDS", 3)
local SCENES = probe.getenv_num("LEGAIA_SCENES", 0) == 1
local WALK_SEED = probe.getenv_num("LEGAIA_WALK_SEED", 0)
local lcg = WALK_SEED
local function rnd(n)
    lcg = (lcg * 1103515245 + 12345) % 2147483648
    return math.floor(lcg / 65536) % n
end
local held2 = nil
local WANT = {}
for f in string.gmatch(probe.getenv("LEGAIA_WANT", ""), "[^;]+") do
    local ids = {}
    for m in string.gmatch(f, "[^+]+") do ids[#ids + 1] = tonumber(m) end
    WANT[#WANT + 1] = ids
end
local WANT_FORCE = probe.getenv_num("LEGAIA_WANT_FORCE", 0) == 1
local WANT_WIDE = probe.getenv_num("LEGAIA_WANT_WIDE", 0) == 1
local TOUGH = probe.getenv_num("LEGAIA_TOUGH", 0) >= 1
local TOUGH_POOL = probe.getenv_num("LEGAIA_TOUGH", 0) == 2
local WOUND = probe.getenv_num("LEGAIA_WOUND", 0)
local WOUND_EVERY = probe.getenv_num("LEGAIA_WOUND_EVERY", 0) ~= 0
local FINISH = probe.getenv_num("LEGAIA_FINISH", 0) == 1
local HOLD_MODULE = probe.getenv_num("LEGAIA_HOLD_MODULE", 0)
local HOLD_MAGIC = probe.getenv_num("LEGAIA_HOLD_MAGIC", 0)
local SAVE_PRE = probe.getenv_num("LEGAIA_SAVE_PRE", 0) == 1
local SCENE_SETTLE = probe.getenv_num("LEGAIA_SCENE_SETTLE", 150)
local LOADED_DEFINE = 0x80084540
local PLAYER_PTR = 0x8007c364

local GAME_MODE_VA = 0x8007b83c
local SCENE_VA     = 0x8007050c
local CTX_PTR      = 0x8007bd24
local ACTORS       = 0x801c9370
local MONSTER_IDS  = 0x8007bd0c

local BTN = probe.BTN
local WALK = { BTN.UP, BTN.RIGHT, BTN.DOWN, BTN.LEFT }
local DIAG = { { BTN.UP, BTN.RIGHT }, { BTN.RIGHT, BTN.DOWN }, { BTN.DOWN, BTN.LEFT }, { BTN.LEFT, BTN.UP } }

local log_lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    log_lines[#log_lines + 1] = s
    PCSX.log("[harvest] " .. s)
end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_VA + i)
        if b == nil or b < 0x20 or b >= 0x7f then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function ok_ptr(p) return p ~= nil and p >= 0x80000000 and p < 0x80200000 end

-- The mid-action states a capture waits in: the strike loop 0x1E, the cast's
-- sustained animation / hit loop 0x2B..0x2C, the module's running phases
-- 0x6F..0x70 (docs/subsystems/battle-action.md, state table).
local function band(st)
    if st == 0x1E then return "attack" end
    if st == 0x2B or st == 0x2C then return "magic" end
    -- A heal-class spell runs the Spirit / Item flow (FUN_801E2EFC's 0x3C arm).
    if st == 0x3E or st == 0x3F then return "magic" end
    if st == 0x6F or st == 0x70 then return "module" end
    return nil
end

local held = nil        -- the button currently forced
local held_until = 0
local function press(btn, vsync, hold)
    if held ~= nil then probe.pad.release(held) end
    probe.pad.force(btn)
    held = btn
    held_until = vsync + (hold or 4)
end
local function release_if_due(vsync)
    if held ~= nil and vsync >= held_until then
        probe.pad.release(held)
        held = nil
    end
end

local function save_checkpoint(name, vsync)
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(probe.out_path(name .. ".rawsstate"), "TRUNCATE")
        fh:writeMoveSlice(w)
        fh:close()
    end)
    logf("CKPT %s vsync=%d ok=%s %s", name, vsync, tostring(ok), tostring(err or ""))
    return ok
end

-- Point the live scene's encounter tables at the wanted formation. The tables
-- are the MAN sections FUN_801D9E1C reads: ctrl = *0x801C6EA4, formations at
-- ctrl[+0x20] (count byte, then rows of stride ctrl[+0x5D]), regions at
-- ctrl[+0x28] (count byte, then rows of stride ctrl[+0x5F]).
local CTRL_PTR = 0x801C6EA4
local STEP_COUNTER = 0x8007B5FC
local want_i = 1
local want_armed = nil
local function arm_want(ids, vsync)
    local c = probe.read_u32(CTRL_PTR)
    if not ok_ptr(c) then return false end
    local ftab, rtab = probe.read_u32(c + 0x20), probe.read_u32(c + 0x28)
    if not ok_ptr(ftab) or not ok_ptr(rtab) then return false end
    local fcount, fstr = probe.read_u8(ftab), probe.read_u8(c + 0x5D)
    local rcount, rstr = probe.read_u8(rtab), probe.read_u8(c + 0x5F)
    if fcount == 0 or rcount == 0 or fstr < 8 or rstr < 8 then return false end
    local row = nil
    for r = 0, fcount - 1 do
        local base = ftab + 1 + r * fstr
        if probe.read_u8(base + 3) == #ids then
            local same = true
            for i, m in ipairs(ids) do
                if probe.read_u8(base + 3 + i) ~= m then same = false end
            end
            if same then row = r; break end
        end
    end
    if row == nil then
        if not WANT_FORCE then return nil end
        local base = ftab + 1
        for i = 0, 3 do probe.write_u8(base + i, 0) end
        probe.write_u8(base + 3, #ids)
        for i = 0, 3 do probe.write_u8(base + 4 + i, ids[i + 1] or 0) end
        row = 0
        logf("vsync=%d synthetic formation over row 0", vsync)
    end
    for r = 0, rcount - 1 do
        local base = rtab + 1 + r * rstr
        if WANT_WIDE then
            probe.write_u8(base + 0, 0)
            probe.write_u8(base + 1, 0)
            probe.write_u8(base + 2, 0xFF)
            probe.write_u8(base + 3, 0xFF)
        end
        probe.write_u8(base + 4, 0x40)
        probe.write_u8(base + 6, row)
        probe.write_u8(base + 7, 1)
    end
    logf("vsync=%d scene=%q want %d: row %d of %d, %d regions", vsync, scene_name(), want_i, row,
        fcount, rcount)
    return true
end

local function seat_actor(s)
    local ap = probe.read_u32(ACTORS + s * 4)
    if ok_ptr(ap) then return ap end
    return nil
end

-- The round-prompt writes (LEGAIA_TOUGH, LEGAIA_WOUND, LEGAIA_FINISH).
local function round_pokes(round)
    if TOUGH then
        for s = 0, 2 do
            local a = seat_actor(s)
            if a ~= nil and (probe.read_u16(a + 0x14E) or 0) > 0 then
                -- All three: state 0x51 parks on a readout +0x172 that
                -- disagrees with the live HP and the pending delta +0x10.
                if TOUGH_POOL then probe.write_u16(a + 0x14E, 9999) end
                local hp = probe.read_u16(a + 0x14E)
                probe.write_u16(a + 0x14C, hp)
                probe.write_u16(a + 0x172, hp)
                probe.write_u32(a + 0x10, 0)
            end
        end
    end
    for s = 3, 7 do
        local a = seat_actor(s)
        if a ~= nil and (probe.read_u16(a + 0x14C) or 0) > 0 then
            if FINISH and round > SPIRIT_ROUNDS then
                probe.write_u16(a + 0x14C, 1)
            elseif round == WOUND or (WOUND_EVERY and WOUND > 0 and round > WOUND) then
                probe.write_u16(a + 0x14C, math.max(1, math.floor(probe.read_u16(a + 0x14E) / 3)))
            end
        end
    end
end

local ring_i = 0
local held_pre = nil
local pre_written = {}
local last_st = nil

local caps = 0
local per_key = {}
local band_key, band_since = nil, 0
local last_mode, last_flow = nil, nil
local next_menu_press = 0
local walk_i, walk_next = 1, 0
local battles = 0
local rounds = 0
local scene_seen = {}
local scene_key, scene_since, scene_pos, scene_moved = nil, 0, nil, false

local audio_log = nil
local function log_audio(vsync, mode)
    local function src(base)
        local seq = probe.read_u16(base + 0xA) or 0
        local rec = probe.read_u32(0x801CD2C0 + (seq % 32) * 4) or 0
        local flags, wait, cursor, vol = -1, -1, -1, -1
        if ok_ptr(rec) then
            flags = probe.read_u32(rec + 0x98) or -1
            wait = probe.read_u32(rec + 0x90) or -1
            cursor = probe.read_u32(rec) or -1
            vol = probe.read_u16(rec + 0x58) or -1
        end
        return string.format("%d,%d,%d,%d,%d,%d", probe.read_u16(base + 8) or 0, seq, flags, wait, cursor, vol)
    end
    local music, cue = 0, 0
    for v = 0, 23 do
        local r = 0x801CDB50 + v * 0x36
        if (probe.read_u16(r + 6) or 0) ~= 0 then
            if (probe.read_u16(r + 0x10) or 0) == 0x21 then cue = cue + 1 else music = music + 1 end
        end
    end
    audio_log:row("%d,%d,%d,%d,%d,%s,%s,%d,%d", vsync, mode, probe.read_u32(0x8007BAC8) or 0,
        probe.read_u32(0x8007BAB8) or 0, probe.read_u32(0x8007B750) or 0,
        src(0x8007052C), src(0x8007056C), music, cue)
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        logf("sstate=%s frames=%d hold=%d per_key=%d max=%d",
            SSTATE_PATH, FRAMES, HOLD, PER_KEY, MAX_CAPS)
        return {}
    end,

    on_capture = function(ctx, vsync)
        release_if_due(vsync)
        local mode = probe.read_u8(GAME_MODE_VA) or 0
        if probe.getenv_num("LEGAIA_AUDIO_LOG", 0) == 1 then
            audio_log = audio_log or probe.csv_open(probe.out_path("audio.csv"),
                "v,mode,word,req,flags,f_open,f_seq,f_flags,f_wait,f_cursor,f_vol,b_open,b_seq,b_flags,b_wait,b_cursor,b_vol,music,cue")
            log_audio(vsync, mode)
        end
        if vsync % 600 == 0 then
            local cp = probe.read_u32(CTX_PTR)
            logf("beat vsync=%d mode=0x%02x round=%d flow=0x%02x st=0x%02x seat=%d", vsync, mode, rounds,
                ok_ptr(cp) and probe.read_u8(cp + 6) or 0, ok_ptr(cp) and probe.read_u8(cp + 7) or 0,
                ok_ptr(cp) and probe.read_u8(cp + 0x13) or 0)
        end
        if mode ~= last_mode then
            logf("vsync=%d game_mode=0x%02x scene=%q", vsync, mode, scene_name())
            if mode == 0x14 or mode == 0x15 then
                if last_mode ~= 0x14 and last_mode ~= 0x15 then
                    battles = battles + 1
                    rounds = 0
                    if want_armed == want_i then want_i = want_i + 1 end
                end
            end
            last_mode = mode
        end

        if mode == 0x03 and SCENES and caps < MAX_CAPS then
            local label = scene_name()
            local key = label .. "/" .. tostring(probe.read_u16(LOADED_DEFINE) or 0)
            local pp = probe.read_u32(PLAYER_PTR)
            local pos = nil
            if ok_ptr(pp) then
                pos = string.format("%d,%d", probe.read_u16(pp + 0x14) or 0, probe.read_u16(pp + 0x18) or 0)
            end
            if key ~= scene_key then
                scene_key, scene_since, scene_pos, scene_moved = key, vsync, pos, false
            elseif pos ~= scene_pos then
                scene_moved = true
            end
            if not scene_seen[label] and scene_moved and vsync - scene_since >= SCENE_SETTLE then
                if save_checkpoint(string.format("%s_scene_%s_v%d", PREFIX, label, vsync), vsync) then
                    scene_seen[label] = true
                    caps = caps + 1
                end
            end
        elseif mode ~= 0x03 then
            scene_key = nil
        end
        if mode ~= 0x03 and held2 ~= nil then probe.pad.release(held2); held2 = nil end
        if mode == 0x03 and #WANT > 0 then
            if want_i > #WANT then
                logf("vsync=%d last formation done", vsync)
                ctx.request_quit = true
                return
            end
            if want_armed ~= want_i then
                local r = arm_want(WANT[want_i], vsync)
                if r == nil then
                    logf("vsync=%d scene=%q holds no row for want %d: skipped", vsync, scene_name(), want_i)
                    want_i = want_i + 1
                    return
                elseif r then
                    want_armed = want_i
                end
            end
            if want_armed == want_i then probe.write_u32(STEP_COUNTER, 1) end
        end
        if mode == 0x03 then
            -- Walk: hold one direction per leg, a Cross now and then.
            if vsync >= walk_next and WALK_SEED ~= 0 then
                if held2 ~= nil then probe.pad.release(held2); held2 = nil end
                local leg = 30 + rnd(121)
                local d = rnd(8)
                walk_next = vsync + leg
                if d < 4 then
                    press(WALK[d + 1], vsync, leg - 2)
                else
                    press(DIAG[d - 3][1], vsync, leg - 2)
                    held2 = DIAG[d - 3][2]
                    probe.pad.force(held2)
                end
            elseif vsync >= walk_next then
                walk_i = (walk_i % #WALK) + 1
                walk_next = vsync + WALK_LEG
                press(WALK[walk_i], vsync, WALK_LEG - 2)
            elseif vsync % 97 == 0 and held == nil then
                press(BTN.CROSS, vsync, 4)
            end
            return
        end
        if mode ~= 0x14 and mode ~= 0x15 then
            if vsync >= next_menu_press then
                -- A save point's menu (modes 0x16 / 0x17) is backed out of
                -- with the cancel buttons; anything else is a box to clear.
                if mode == 0x16 or mode == 0x17 then
                    press(math.floor(vsync / 30) % 2 == 0 and BTN.TRIANGLE or BTN.CIRCLE, vsync, 4)
                else
                    press(BTN.CROSS, vsync, 4)
                end
                next_menu_press = vsync + 30
            end
            return
        end

        local cp = probe.read_u32(CTX_PTR)
        if not ok_ptr(cp) then return end
        local flow = probe.read_u8(cp + 6) or 0
        local st = probe.read_u8(cp + 7) or 0
        local seat = probe.read_u8(cp + 0x13) or 0
        if flow ~= last_flow then
            if flow == 0x1E then
                rounds = rounds + 1
                round_pokes(rounds)
            end
            last_flow = flow
            next_menu_press = vsync + 10
        end

        if flow == 0xFF then
            -- The action SM runs: watch for a monster's action on screen.
            local b = band(st)
            if SAVE_PRE and st == 0x0A and last_st ~= 0x0A then
                held_pre = PCSX.createSaveState()
            end
            last_st = st
            if seat >= 3 and seat < 8 and b ~= nil then
                local ap = probe.read_u32(ACTORS + seat * 4)
                if ok_ptr(ap) then
                    local mid = probe.read_u8(MONSTER_IDS + seat - 3) or 0
                    local cat = probe.read_u8(ap + 0x1DE) or 0
                    local q = probe.read_u8(ap + 0x1DF) or 0
                    -- A physical attack's queue byte is a swing, not an
                    -- attack id: key it on the monster alone.
                    if cat == 3 then q = 0 end
                    local key = string.format("m%03d_c%d_q%02x", mid, cat, q)
                    if band_key ~= key .. b then
                        band_key, band_since = key .. b, vsync
                        logf("action %s band=%s st=0x%02x seat=%d vsync=%d", key, b, st, seat, vsync)
                    elseif SAVE_PRE and cat == 2 and held_pre ~= nil and not pre_written[key] then
                        pre_written[key] = true
                        local name = string.format("%s_%s_%s_pre_v%d", PREFIX, key, scene_name(), vsync)
                        local fh = Support.File.open(probe.out_path(name .. ".rawsstate"), "TRUNCATE")
                        fh:writeMoveSlice(held_pre)
                        fh:close()
                        held_pre = nil
                        logf("CKPT %s vsync=%d", name, vsync)
                    elseif vsync - band_since >= (b == "module" and HOLD_MODULE > 0 and HOLD_MODULE
                            or b == "magic" and HOLD_MAGIC > 0 and HOLD_MAGIC or HOLD)
                        and (per_key[key] or 0) < PER_KEY and caps < MAX_CAPS then
                        local name = string.format("%s_%s_%s_st%02x_v%d",
                            PREFIX, key, scene_name(), st, vsync)
                        if save_checkpoint(name, vsync) then
                            per_key[key] = (per_key[key] or 0) + 1
                            caps = caps + 1
                            if caps >= MAX_CAPS then ctx.request_quit = true end
                        end
                    end
                end
            else
                band_key = nil
            end
            return
        end
        band_key = nil
        if vsync < next_menu_press then return end
        if flow == 0x28 then
            if rounds > SPIRIT_ROUNDS then
                -- The last formation's fight has nothing left to give.
                if #WANT > 0 and want_i > #WANT then ctx.request_quit = true end
                -- A seat whose gauge refuses one arm parks a Left-only drive.
                local arms = { BTN.LEFT, BTN.RIGHT, BTN.UP, BTN.CROSS }
                ring_i = ring_i % #arms + 1
                press(arms[ring_i], vsync, 4)
            else
                press(BTN.DOWN, vsync, 4)
            end
        else
            press(BTN.CROSS, vsync, 4)
        end
        next_menu_press = vsync + 20
    end,

    on_done = function(_, _)
        if held ~= nil then probe.pad.release(held) end
        if held2 ~= nil then probe.pad.release(held2) end
        logf("done: battles=%d captures=%d", battles, caps)
        local fh = io.open(probe.out_path("harvest.log"), "w")
        if fh ~= nil then
            fh:write(table.concat(log_lines, "\n") .. "\n")
            fh:close()
        end
    end,
})
