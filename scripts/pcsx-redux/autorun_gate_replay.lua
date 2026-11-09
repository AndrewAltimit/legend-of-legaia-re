-- autorun_gate_replay.lua
--
-- The retail side of the gate replay (scripts/ci/gate-replay.py): load a state
-- and play an engine run's pad back beat by beat, each beat's presses relative
-- to its own opening gate, waiting for retail at each closing gate rather than
-- running on one clock. Poll-only, safe under --fast.
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_OUT_DIR,
--      LEGAIA_GATE_PLAN         the plan the driver wrote, see below.
--      LEGAIA_GATE_SYNTH_AFTER  vsyncs of waiting before a synthetic Cross
--                               (default 90; 0 = never press).
--      LEGAIA_GATE_MAX_WAIT     vsyncs a beat may wait before the run stops
--                               (default 1800).
--      LEGAIA_GATE_REPULSE_MAX  extra pulses a waiting battle beat may add
--                               (default 16; 0 = none).
--
-- The plan is text, one record a line:
--   BEAT idx kind value length   a beat of `length` engine ticks closing on
--                                gate `kind value`. Kinds: engaged 0|1,
--                                battle 0|1, scene NAME, warp -, pos X/Z,
--                                page N (the Nth page end since the last gate
--                                that was not a page or waypoint), act XX:N
--                                (the Nth time the action state ctx+0x07 entered
--                                XX in this battle), flow XX:N (the same for the
--                                command flow state ctx+0x06), end -.
--   PAD  rel 0xMASK              the held word from `rel` ticks into the beat
--                                (bit N of the mask is probe.BTN index N).
--   SEEK X/Z +x -x +z -z         where the engine stood when this beat's gate
--                                fired, and the d-pad masks that walk each way.
--
-- What happens at a closing gate:
--   retail first  the beat ends and the rest of its presses are dropped, and
--                 the count is recorded.
--   retail late   the presses have run out, so the beat waits. A walking
--                 player walks to the beat's SEEK spot if it has one (a door or
--                 ledge fires on where the player stands, and the two walks
--                 differ by a pass of position), else keeps the beat's last
--                 d-pad held; an engaged player holds nothing. The seek is
--                 synthetic input, logged as such. Past LEGAIA_GATE_SYNTH_AFTER
--                 vsyncs an engaged beat gets a short Cross every 30 vsyncs
--                 (a page retail still shows is the usual cause), logged as
--                 synthetic with its vsync, in the beat table and the event
--                 list: input the engine's log did not supply is a finding.
--
-- Levels match rather than edges, so two gates the sides reach in the other
-- order read as one zero-length beat. A warp has no level: it is latched and
-- lapses after WARP_LAPSE vsyncs. Retail holds the engaged bit across a door's whole
-- run (trigger to landing) where the engine's engaged word stays clear, so a
-- retail engaged interval that contains a warp is the door's hold and never
-- satisfies `engaged 1`; the hold ends where the bit next falls.
--
-- A `pos` gate is a waypoint on the engine's walk. It closes within POS_NEAR of
-- the engine's spot, or - once the beat's pad has run out and retail has come
-- within POS_PASS - where the distance starts growing again (status `passed`).
-- Retail engaging closes it at once (status `cut`): the walk is over, so the
-- waypoints left in it would only wait on nothing.
--
-- A `page` gate counts rising edges of the pager state `_DAT_801F2734` reaching
-- 0x19 (a page shown whole, waiting for a press), so a press lands relative to
-- the page it turns rather than to the start of the talk. The count is by
-- ordinal, so a page the sides reach in another order or number resynchronises
-- at the next state gate. A beat waiting on a page that is already shown gets
-- its synthetic Cross sooner (SYNTH_PAGE_AFTER vsyncs).
--
-- `act` and `flow` gates come from the battle's two state bytes, which the
-- port mirrors: the commit state 0x0A of the action SM and the round prompt
-- 0x1E of the command flow, so a prompt's presses land relative to the prompt
-- they answer. In battle a late beat holds no input, and the synthetic Cross
-- rule applies to it as to an engaged beat.
--
-- A beat closing on an `act` or `flow` gate whose presses have run out, with the
-- command flow on a byte that waits for input, repeats the beat's last press
-- until the byte moves: a two-vsync pulse every REPULSE_PERIOD vsyncs, none for
-- REPULSE_SETTLE vsyncs after the byte last moved (the ring ignores input that
-- long on entry), at most LEGAIA_GATE_REPULSE_MAX to a beat. Each pulse is
-- logged `synthetic_repulse` and counted as synthetic. The synthetic Cross waits
-- until the pulses are spent.
--
-- Outputs: gate_beats.csv (one row a beat), gate_replay.csv (one row a vsync),
-- gate_events.csv (every synthetic press and unexpected gate).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 12000)
local PLAN = probe.getenv("LEGAIA_GATE_PLAN", "")
local SYNTH_AFTER = probe.getenv_num("LEGAIA_GATE_SYNTH_AFTER", 90)
local MAX_WAIT = probe.getenv_num("LEGAIA_GATE_MAX_WAIT", 1800)

local PLAYER_PTR = 0x8007C364
local GAME_MODE  = 0x8007B83C
local BATTLE_CTX = 0x8007BD24
local RAND_SEED  = 0x80009010
local SCENE_NAME = 0x8007050C
local BATTLE_MODE = 0x15
local ENGAGED_BIT = 0x80000
local WARP_DIST = 300
local WARP_LAPSE = 30
local SEEK_NEAR = 6
local POS_NEAR, POS_PASS, POS_SLACK = 24, 160, 24
local PAGER_STATE = 0x801F2734
local PAGE_WAIT = 0x19
local SYNTH_PAGE_AFTER = 30
local FIELD_MODE = 3
local CROSS = probe.BTN.CROSS
local SYNTH_PERIOD, SYNTH_HOLD = 30, 2
local REPULSE_MAX = probe.getenv_num("LEGAIA_GATE_REPULSE_MAX", 16)
local REPULSE_PERIOD, REPULSE_SETTLE = 8, 18
-- ctx[+0x06] bytes that sit waiting for a press: the prompt, ring, escape, item,
-- magic, arts, target, target confirm and attack-mode states.
local INPUT_FLOW = { [0x1E] = true, [0x28] = true, [0x32] = true, [0x3C] = true,
    [0x46] = true, [0x50] = true, [0x5A] = true, [0x64] = true, [0x78] = true }

local function s16(a)
    local v = probe.read_u16(a)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local function read_scene()
    local s = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME + i)
        if b < 0x20 or b >= 0x7f then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end

-- The plan.
local beats = {}
if PLAN ~= "" then
    for line in io.lines(PLAN) do
        local a, b, c, d, e = string.match(line, "^BEAT (%d+) (%S+) (%S+) (%d+)()")
        if a then
            local beat = { idx = tonumber(a), kind = b, value = c, len = tonumber(d), pad = {} }
            if b == "pos" then
                local x, z = string.match(c, "^(-?%d+)/(-?%d+)$")
                beat.x, beat.z = tonumber(x), tonumber(z)
            end
            beats[#beats + 1] = beat
        else
            local rel, mask = string.match(line, "^PAD (%d+) 0[xX](%x+)")
            if rel and #beats > 0 then
                local t = beats[#beats].pad
                t[#t + 1] = { tonumber(rel), tonumber(mask, 16) }
            end
            local sx, sz, px, nx, pz, nz = string.match(line,
                "^SEEK (-?%d+)/(-?%d+) (%d+) (%d+) (%d+) (%d+)")
            if sx and #beats > 0 then
                beats[#beats].seek = { x = tonumber(sx), z = tonumber(sz), px = tonumber(px),
                    nx = tonumber(nx), pz = tonumber(pz), nz = tonumber(nz) }
            end
        end
    end
end
assert(#beats > 0, "gate replay: no beats in plan " .. PLAN)

-- The bits newly down in `mask` given the word before it.
local function rising(mask, before)
    local r = 0
    for bit = 0, 15 do
        local f = 2 ^ bit
        if math.floor(mask / f) % 2 == 1 and math.floor(before / f) % 2 == 0 then r = r + f end
    end
    return r
end

-- Each beat's last press: what a waiting battle beat repeats.
for _, b in ipairs(beats) do
    local before = 0
    for _, row in ipairs(b.pad) do
        local edge = rising(row[2], before)
        if edge ~= 0 then b.press = edge end
        before = row[2]
    end
end
PCSX.log(string.format("[gate-replay] %d beats from %s", #beats, PLAN))

local fbeats = assert(io.open(probe.out_path("gate_beats.csv"), "w"))
fbeats:write("beat,kind,value,engine_len,vsync_start,vsync_end,waited,dropped_presses,synthetic,status,x,z,rng,mode,scene\n")
local frow = assert(io.open(probe.out_path("gate_replay.csv"), "w"))
frow:write("vsync,beat,rel,waiting,x,y,z,engaged,battle,mode,scene,pad,rng,synth,pager,act,flow\n")
local fev = assert(io.open(probe.out_path("gate_events.csv"), "w"))
fev:write("vsync,beat,event,detail\n")

local function event(el, b, what, detail)
    fev:write(string.format("%d,%d,%s,%s\n", el, b, what, detail or ""))
    fev:flush()
end

local cur = 1               -- index in `beats`
local beat_start = 0        -- vsync the current beat opened on
local pad_i, pad_mask = 1, 0
local synth_count, last_press = 0, -1000
local warp_at, last_x, last_z = nil, nil, nil
local door_hold = false
local pager_prev, pages_seen = nil, 0
local act_prev, act_counts = nil, {}
local flow_prev, flow_counts = nil, {}
local flow_moved_at = 0
local held, done = {}, false

local function press_dropped(b, from_i)
    local n = 0
    for i = from_i, #b.pad do
        if math.floor(b.pad[i][2] / 2 ^ CROSS) % 2 == 1 then n = n + 1 end
    end
    return n
end

local function dpad(mask)
    local r = 0
    for bit = 4, 7 do
        if math.floor(mask / 2 ^ bit) % 2 == 1 then r = r + 2 ^ bit end
    end
    return r
end

-- The d-pad word that walks toward the seek spot, 0 once on it.
local function seek_mask(seek, s)
    local mask = 0
    local dx, dz = seek.x - s.x, seek.z - s.z
    if math.abs(dx) > SEEK_NEAR then mask = mask + (dx > 0 and seek.px or seek.nx) end
    if math.abs(dz) > SEEK_NEAR then mask = mask + (dz > 0 and seek.pz or seek.nz) end
    return mask
end

local function apply(mask)
    for bit = 0, 15 do
        local on = math.floor(mask / 2 ^ bit) % 2 == 1
        if on and not held[bit] then probe.pad_force(bit); held[bit] = true
        elseif not on and held[bit] then probe.pad_release(bit); held[bit] = nil end
    end
end

local function snapshot()
    local p = probe.read_u32(PLAYER_PTR)
    local s = { x = -32768, y = -32768, z = -32768, engaged = 0 }
    if p ~= nil and probe.in_ram(p) then
        s.x, s.y, s.z = s16(p + 0x14), s16(p + 0x16), s16(p + 0x18)
        s.engaged = math.floor(probe.read_u32(p + 0x10) / ENGAGED_BIT) % 2
    end
    s.mode = probe.read_u32(GAME_MODE) % 256
    local ctx = probe.read_u32(BATTLE_CTX)
    s.battle = (s.mode == BATTLE_MODE or ctx ~= 0) and 1 or 0
    local in_ctx = s.battle == 1 and probe.in_ram(ctx)
    s.act = in_ctx and probe.read_u8(ctx + 7) or -1
    s.flow = in_ctx and probe.read_u8(ctx + 6) or -1
    s.scene = read_scene()
    s.rng = probe.read_u32(RAND_SEED)
    s.pager = probe.read_u32(PAGER_STATE)
    return s
end

-- The gate's status when `b` has closed this vsync, else nil.
local function gate_status(b, s, el)
    if b.kind == "engaged" then
        if b.value == "1" and door_hold then return nil end
        return s.engaged == tonumber(b.value) and "gate" or nil
    end
    if b.kind == "battle" then return s.battle == tonumber(b.value) and "gate" or nil end
    if b.kind == "scene" then return s.scene == b.value and "gate" or nil end
    if b.kind == "act" or b.kind == "flow" then
        local v, n = string.match(b.value, "^(%x+):(%d+)$")
        local counts = b.kind == "act" and act_counts or flow_counts
        return (counts[tonumber(v, 16)] or 0) >= tonumber(n) and "gate" or nil
    end
    if b.kind == "page" then return pages_seen >= tonumber(b.value) and "gate" or nil end
    if b.kind == "warp" then
        return (warp_at ~= nil and el - warp_at <= WARP_LAPSE) and "gate" or nil
    end
    if b.kind == "pos" and s.x ~= -32768 then
        if s.engaged == 1 and not door_hold then return "cut" end
        local d = math.abs(s.x - b.x) + math.abs(s.z - b.z)
        if d <= POS_NEAR then return "gate" end
        if not b.nearest or d < b.nearest then b.nearest = d end
        if el - beat_start >= b.len and b.nearest <= POS_PASS and d > b.nearest + POS_SLACK then
            return "passed"
        end
    end
    return nil
end

-- A waiting battle beat's repeated press: the mask to hold this vsync (0 between
-- pulses), or nil when the beat is not repeating one.
local function repulse(b, s, el, wait)
    local eligible = b.press and (b.kind == "flow" or b.kind == "act") and INPUT_FLOW[s.flow]
    local pulses = b.pulses or 0
    if not eligible or REPULSE_MAX <= 0 then return nil end
    if b.pulse_at and el - b.pulse_at < SYNTH_HOLD then return b.press end
    if pulses >= REPULSE_MAX then return nil end
    if wait >= REPULSE_PERIOD and el - flow_moved_at >= REPULSE_SETTLE
        and (b.pulse_at == nil or el - b.pulse_at >= REPULSE_PERIOD) then
        b.pulses, b.pulse_at = pulses + 1, el
        synth_count = synth_count + 1
        event(el, b.idx, "synthetic_repulse",
            string.format("wait=%d press=%04x flow=%02x", wait, b.press, s.flow))
        return b.press
    end
    return 0
end

local function close_beat(el, s, status)
    local b = beats[cur]
    local waited = math.max(0, el - beat_start - b.len)
    fbeats:write(string.format("%d,%s,%s,%d,%d,%d,%d,%d,%d,%s,%d,%d,%08x,%d,%s\n",
        b.idx, b.kind, b.value, b.len, beat_start, el, waited,
        press_dropped(b, pad_i), synth_count, status, s.x, s.z, s.rng, s.mode, s.scene))
    fbeats:flush()
    if b.kind == "warp" then warp_at = nil end
    if b.kind ~= "page" and b.kind ~= "pos" then pages_seen = 0 end
    if b.kind == "battle" then act_counts, flow_counts = {}, {} end
    cur = cur + 1
    beat_start, pad_i, pad_mask, synth_count, last_press = el, 1, 0, 0, -1000
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_gate_replay.lua",
            { sstate = SSTATE, frames = FRAMES, plan = PLAN })
        return {}
    end,
    on_capture = function(ctx, el)
        if done then return end
        local s = snapshot()
        if s.x ~= -32768 then
            if last_x and math.abs(s.x - last_x) + math.abs(s.z - last_z) > WARP_DIST then
                warp_at = el
                if s.engaged == 1 and not door_hold then
                    door_hold = true
                    event(el, beats[math.min(cur, #beats)].idx, "door_hold", "")
                end
            end
            last_x, last_z = s.x, s.z
        end
        if s.engaged == 0 then door_hold = false end
        if s.battle == 0 and s.pager == PAGE_WAIT and pager_prev ~= PAGE_WAIT then
            pages_seen = pages_seen + 1
        end
        pager_prev = s.pager
        if s.act ~= -1 and s.act ~= act_prev then
            act_counts[s.act] = (act_counts[s.act] or 0) + 1
        end
        act_prev = s.act
        if s.flow ~= -1 and s.flow ~= flow_prev then
            flow_counts[s.flow] = (flow_counts[s.flow] or 0) + 1
            flow_moved_at = el
        end
        flow_prev = s.flow

        -- Close every gate this vsync reached.
        while cur <= #beats do
            local status = gate_status(beats[cur], s, el)
            if status == nil then break end
            close_beat(el, s, status)
        end

        if cur > #beats then
            apply(0)
            done, ctx.request_quit = true, true
            return
        end

        local b = beats[cur]
        local rel = el - beat_start
        if rel >= b.len and b.kind == "end" then
            close_beat(el, s, "end")
            apply(0)
            done, ctx.request_quit = true, true
            return
        end

        local wait = rel - b.len
        local waiting = wait >= 0
        local mask, synth = 0, 0
        if not waiting then
            while pad_i <= #b.pad and b.pad[pad_i][1] <= rel do
                pad_mask = b.pad[pad_i][2]
                pad_i = pad_i + 1
            end
            mask = pad_mask
        else
            -- The presses have run out and the gate has not come.
            if wait >= MAX_WAIT then
                event(el, b.idx, "timeout", b.kind .. " " .. b.value)
                close_beat(el, s, "timeout")
                apply(0)
                done, ctx.request_quit = true, true
                return
            end
            if s.engaged == 0 and s.x ~= -32768 and s.mode == FIELD_MODE and s.battle == 0 then
                if b.seek then
                    mask = seek_mask(b.seek, s)
                    synth = mask ~= 0 and 1 or 0
                    if synth == 1 and not b.seeking then
                        b.seeking = true
                        synth_count = synth_count + 1
                        event(el, b.idx, "synthetic_seek",
                            string.format("from %d/%d to %d/%d", s.x, s.z, b.seek.x, b.seek.z))
                    end
                else
                    mask = dpad(pad_mask)
                end
            end
            local pulse = repulse(b, s, el, wait)
            if pulse ~= nil and pulse ~= 0 then
                mask, synth = pulse, 1
            end
            local after = s.pager == PAGE_WAIT and math.min(SYNTH_AFTER, SYNTH_PAGE_AFTER) or SYNTH_AFTER
            if pulse == nil and (s.engaged == 1 or s.battle == 1) and SYNTH_AFTER > 0 and wait >= after then
                local phase = (wait - after) % SYNTH_PERIOD
                if phase == 0 then
                    synth_count = synth_count + 1
                    event(el, b.idx, "synthetic_cross", string.format("wait=%d", wait))
                end
                if phase < SYNTH_HOLD then
                    mask = 2 ^ CROSS
                    synth = 1
                end
            end
        end
        apply(mask)
        frow:write(string.format("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%s,%04x,%08x,%d,%x,%d,%d\n",
            el, b.idx, rel, waiting and 1 or 0, s.x, s.y, s.z, s.engaged, s.battle,
            s.mode, s.scene, mask, s.rng, synth, s.pager, s.act, s.flow))
        frow:flush()
    end,
})
