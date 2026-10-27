-- autorun_walk_route.lua
--
-- Walk a planned route through one or more field scenes on the retail disc,
-- pad only, and save a raw state in each scene the route enters and at the
-- route's end. The retail-capture tool for a place no save state stands in:
-- a minigame cabinet, a far room, a scene behind three doors.
--
-- Pure retail play: no RAM writes, no pokes. The pad is the only input.
--
-- The route is data (LEGAIA_ROUTE_FILE), one waypoint per line:
--
--   <scene> <x> <z> [tp]      walk to world (x, z) while <scene> is loaded
--   <scene> WAIT <n> 0        stand still <n> vsyncs
--   <scene> AVOID tx,tz;...   from here on, no press may carry the player
--                             onto these tiles (the scene's other doors;
--                             a sidestep near a door otherwise walks
--                             through it)
--   <scene> TALK <dir> <n>    face <dir> and press Cross (up to <n> tries)
--                             until a dialogue box opens, then page it
--                             until a push toward the next waypoint moves
--                             the player (a guard that steps aside)
--   <scene> ACT <dir> <n>     face world direction <dir> (ring index 0..7,
--                             0 = Z+, 2 = X+, 4 = Z-, 6 = X-) and press
--                             Cross, every 30 vsyncs, up to <n> times or
--                             until the game mode leaves the field (3);
--                             the direction is held on the first
--                             LEGAIA_ACT_FACE (2) presses only, so a
--                             picker the talk opens is not steered
--                             (LEGAIA_ACT_SHOT_EVERY=N also saves a
--                             state every N presses)
--
-- A waypoint is reached within LEGAIA_TOL units (default 20) on both axes, or
-- when the player stops within 2 * LEGAIA_TOL of it. A `tp` waypoint is a
-- kind-0 teleport tile of the scene's .MAP (docs/formats/field-map.md): it is
-- also reached when the player's position jumps. Waypoints of a scene other
-- than the loaded one are skipped forward to the loaded scene's first one, so
-- a door the route walks onto hands over to the next scene's leg. Plan the
-- waypoints offline on the scene's .MAP wall sub-cells and teleport table.
--
-- Steering is closed-loop on the player's +0x14 / +0x18 (*0x8007C364): the
-- world direction toward the waypoint is rounded to the eight-way compass
-- ring and un-rotated by the scene's pad octant gp+0x2D8 (0x8007B5F0, the
-- word field-VM op 0x4C nibble 2 writes; field-locomotion.md, "gp+0x2D8 is
-- authored, not computed"), so the press walks that world direction in any
-- scene. A push that stops the player out of range sidesteps for 16 vsyncs,
-- alternating sides. An open dialogue box (*0x801C6EA4 +0x62) is paged with
-- Cross. A random encounter (mode 0x14 / 0x15) is fought the harvester's
-- way - Attack at the command ring, Cross through every other prompt - and
-- the route resumes in the field; LEGAIA_FIGHT=0 ends the run there instead.
--
-- Checkpoints (<OUT_DIR>/<PREFIX>_...rawsstate):
--   _scene_<label>_v<vsync>   once per scene, LEGAIA_SCENE_SETTLE (150)
--                             vsyncs after it loads, once the player has
--                             walked in it (the harvester's scene rule)
--   _end_v<vsync>             the last waypoint reached (no ACT line)
--   _mode<mm>_t<t>            after an ACT line: LEGAIA_SHOTS vsyncs
--                             ("120,300,600") after the game mode first
--                             leaves the field and stays out of it; with
--                             LEGAIA_MASH=1 Cross is pressed every 20
--                             vsyncs from LEGAIA_MASH_FROM (default: the
--                             first shot), so the later shots land at a
--                             decided moment of a minigame; and
--                             LEGAIA_PLAY_SCRIPT ("t:BTN[:hold],...", t
--                             counted from the same clock) presses the
--                             buttons a minigame's menus need;
--                             LEGAIA_WATCH="0xADDR:value[:delay]" also
--                             saves _watch_... once the u32 at ADDR reads
--                             value (+ delay vsyncs): a minigame's own
--                             state word, for a decided moment
--
-- LEGAIA_PLAY_ONLY=1 skips the route: the state already stands in the
-- minigame, and the shot clock and LEGAIA_PLAY_SCRIPT count from the load.
--
-- Env: LEGAIA_SSTATE, LEGAIA_ROUTE_FILE, LEGAIA_OUT_DIR, LEGAIA_PREFIX
-- (default "route"), LEGAIA_FRAMES (default 6000), LEGAIA_TOL, LEGAIA_SHOTS,
-- LEGAIA_MASH, LEGAIA_MASH_BTN (default CROSS; a comma list takes turns), LEGAIA_SCENE_SETTLE.
-- No breakpoints are armed, so --fast (dynarec) runs are fine.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local ROUTE_FILE  = probe.getenv("LEGAIA_ROUTE_FILE", "")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 6000)
local PREFIX      = probe.getenv("LEGAIA_PREFIX", "route")
local TOL         = probe.getenv_num("LEGAIA_TOL", 20)
local SETTLE      = probe.getenv_num("LEGAIA_SCENE_SETTLE", 150)
local MASH        = probe.getenv_num("LEGAIA_MASH", 0) == 1
local MASH_BTNS = {}
for n in string.gmatch(probe.getenv("LEGAIA_MASH_BTN", "CROSS"), "%a+") do
    MASH_BTNS[#MASH_BTNS + 1] = probe.BTN[n]
end
if #MASH_BTNS == 0 then MASH_BTNS[1] = probe.BTN.CROSS end
local PLAY = {}
for t, b, h in string.gmatch(probe.getenv("LEGAIA_PLAY_SCRIPT", ""), "(%d+):(%a+):?(%d*)") do
    PLAY[#PLAY + 1] = { t = tonumber(t), btn = probe.BTN[b], hold = tonumber(h) or 6 }
end
local FIGHT = probe.getenv_num("LEGAIA_FIGHT", 1) == 1
local PLAY_ONLY = probe.getenv_num("LEGAIA_PLAY_ONLY", 0) == 1
local ACT_FACE = probe.getenv_num("LEGAIA_ACT_FACE", 2)
local ACT_SHOT_EVERY = probe.getenv_num("LEGAIA_ACT_SHOT_EVERY", 0)
local MASH_FROM = probe.getenv_num("LEGAIA_MASH_FROM", -1)
local WATCH = nil
do
    local a, v, d = string.match(probe.getenv("LEGAIA_WATCH", ""), "(0x%x+):(%d+):?(%d*)")
    if a then WATCH = { addr = tonumber(a), value = tonumber(v), delay = tonumber(d) or 0 } end
end
local SHOTS = {}
for t in string.gmatch(probe.getenv("LEGAIA_SHOTS", "120,300,600"), "%d+") do
    SHOTS[#SHOTS + 1] = tonumber(t)
end

local GAME_MODE_VA = 0x8007b83c
local SCENE_VA     = 0x8007050c
local PLAYER_PTR   = 0x8007c364
local PAD_OCTANT   = 0x8007b5f0
local FIELD_CTL    = 0x801c6ea4

local BTN = probe.BTN
-- Compass ring order (FIELD_DIR_RING): Z+, Z+X+, X+, Z-X+, Z-, Z-X-, X-, Z+X-,
-- as pad presses under octant 0 (screen-up walks Z+, screen-right X+).
local RING = {
    { BTN.UP }, { BTN.UP, BTN.RIGHT }, { BTN.RIGHT }, { BTN.DOWN, BTN.RIGHT },
    { BTN.DOWN }, { BTN.DOWN, BTN.LEFT }, { BTN.LEFT }, { BTN.UP, BTN.LEFT },
}

local log_lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    log_lines[#log_lines + 1] = s
    PCSX.log("[route] " .. s)
end

local route = {}
do
    local fh = io.open(ROUTE_FILE, "r")
    if fh then
        for line in fh:lines() do
            local sc, a, b, c = string.match(line, "^(%S+)%s+(%S+)%s+(%S+)%s*(%S*)")
            if sc and not string.match(sc, "^#") then
                if a == "AVOID" then
                    local set = {}
                    for tx, tz in string.gmatch(b, "(%d+),(%d+)") do set[tonumber(tx) * 128 + tonumber(tz)] = true end
                    route[#route + 1] = { scene = sc, avoid = set }
                elseif a == "WAIT" then
                    route[#route + 1] = { scene = sc, wait = tonumber(b) or 60 }
                elseif a == "TALK" then
                    route[#route + 1] = { scene = sc, talk = true, dir = tonumber(b), n = tonumber(c) or 10 }
                elseif a == "ACT" then
                    route[#route + 1] = { scene = sc, act = true, dir = tonumber(b), n = tonumber(c) or 10 }
                else
                    route[#route + 1] = { scene = sc, x = tonumber(a), z = tonumber(b), tp = (c == "tp") }
                end
            end
        end
        fh:close()
    end
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
local function s16(a)
    local v = probe.read_u16(a) or 0
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local held = {}
local function set_pad(list)
    local want = {}
    for _, b in ipairs(list or {}) do want[b] = true end
    for b in pairs(held) do if not want[b] then probe.pad.release(b); held[b] = nil end end
    for b in pairs(want) do if not held[b] then probe.pad.force(b); held[b] = true end end
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

-- Door tiles the current leg must not step on (an AVOID line): a press whose
-- next 64 units land on one is not made.
local avoid = {}
local RING_VEC = { { 0, 1 }, { 1, 1 }, { 1, 0 }, { 1, -1 }, { 0, -1 }, { -1, -1 }, { -1, 0 }, { -1, 1 } }
local function into_avoid(px, pz, d)
    local v = RING_VEC[d + 1]
    for _, k in ipairs({ 24, 48, 64 }) do
        local x, z = px + v[1] * k, pz + v[2] * k
        if avoid[math.floor(x / 128) * 128 + math.floor(z / 128)] then return true end
    end
    return false
end

local function ring_dir(dx, dz)
    -- atan2(x, z): Z+ = 0, X+ = quarter turn, ring index in 45-degree steps.
    local a = math.atan2(dx, dz)
    return math.floor(a / (math.pi / 4) + 0.5) % 8
end

local wi = 1
local last_mode, last_scene = nil, nil
local scene_seen, scene_since, scene_start_pos, scene_walked = {}, 0, nil, false
local last_xy, still = nil, 0
local side_until, side_dir, side_flip = 0, nil, false
local act_press, act_v = 0, nil
local out_since, shot_i = nil, 1
local finished = false
local dlg_next = 0
local last_flow = nil

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        logf("sstate=%s route=%s (%d lines) frames=%d", SSTATE_PATH, ROUTE_FILE, #route, FRAMES)
        if #route == 0 and not PLAY_ONLY then ctx.request_quit = true end
        if PLAY_ONLY then act_v = 0 end
        return {}
    end,

    on_capture = function(ctx, vsync)
        if finished then return end
        local mode = probe.read_u8(GAME_MODE_VA) or 0
        local label = scene_name()
        if mode ~= last_mode or label ~= last_scene then
            local pp = probe.read_u32(PLAYER_PTR)
            local px, pz = 0, 0
            if ok_ptr(pp) then px, pz = s16(pp + 0x14), s16(pp + 0x18) end
            logf("vsync=%d game_mode=0x%02x scene=%q player=(%d,%d) wp=%d", vsync, mode, label, px, pz, wi)
            -- A scene's settle clock starts when the field resumes in it
            -- (the label changes a fade before the load finishes).
            if label ~= last_scene or mode == 0x03 then
                scene_since, scene_start_pos, scene_walked = vsync, nil, false
            end
            last_mode, last_scene = mode, label
        end

        -- After an ACT: the game left the field; shoot the mode.
        if act_v ~= nil and mode ~= 0x03 then
            out_since = out_since or vsync
            local t = vsync - out_since
            local want = {}
            for _, p in ipairs(PLAY) do
                if t >= p.t and t < p.t + p.hold then want[#want + 1] = p.btn end
            end
            local from = MASH_FROM >= 0 and MASH_FROM or SHOTS[1]
            if MASH and t >= from and t % 20 < 4 then
                want[#want + 1] = MASH_BTNS[(math.floor(t / 20) % #MASH_BTNS) + 1]
            end
            set_pad(want)
            if WATCH and not WATCH.done then
                if WATCH.hit == nil and (probe.read_u32(WATCH.addr) or -1) == WATCH.value then
                    WATCH.hit = vsync
                end
                if WATCH.hit and vsync - WATCH.hit == WATCH.delay then
                    save_checkpoint(string.format("%s_watch_mode%02x_t%d", PREFIX, mode, t), vsync)
                    WATCH.done = true
                end
            end
            if SHOTS[shot_i] and t == SHOTS[shot_i] then
                save_checkpoint(string.format("%s_mode%02x_t%d", PREFIX, mode, t), vsync)
                shot_i = shot_i + 1
                if not SHOTS[shot_i] then finished = true; ctx.request_quit = true end
            end
            return
        end
        if act_v ~= nil and out_since ~= nil and mode == 0x03 then
            -- Back in the field before every shot: restart the clock.
            out_since = nil
        end

        if mode == 0x14 or mode == 0x15 then
            if not FIGHT then
                set_pad({})
                logf("vsync=%d: battle started; LEGAIA_FIGHT=0 ends the run here", vsync)
                finished = true; ctx.request_quit = true
                return
            end
            -- The harvester's battle hand: Attack at the command ring,
            -- Cross through every other prompt and the results.
            local cp = probe.read_u32(0x8007bd24)
            local flow = ok_ptr(cp) and (probe.read_u8(cp + 6) or 0) or 0
            if flow ~= last_flow then
                logf("vsync=%d battle flow 0x%02x", vsync, flow)
                last_flow = flow
            end
            if flow == 0xFF then set_pad({}); return end
            local t = vsync % 20
            if t == 0 then set_pad({ flow == 0x28 and BTN.LEFT or BTN.CROSS })
            elseif t == 4 then set_pad({}) end
            return
        end
        if mode ~= 0x03 then
            -- Field-side boxes outside mode 3 (a results banner, a save
            -- point's menu) are paged or backed out of; plain loads idle.
            if mode == 0x16 or mode == 0x17 then
                local t = vsync % 30
                if t == 0 then set_pad({ BTN.TRIANGLE }) elseif t == 4 then set_pad({}) end
            else
                set_pad({})
            end
            return
        end

        local pp = probe.read_u32(PLAYER_PTR)
        if not ok_ptr(pp) then return end
        local px, pz = s16(pp + 0x14), s16(pp + 0x18)

        -- Scene checkpoint.
        if scene_start_pos == nil then scene_start_pos = { px, pz } end
        if px ~= scene_start_pos[1] or pz ~= scene_start_pos[2] then scene_walked = true end
        if not scene_seen[label] and scene_walked and vsync - scene_since >= SETTLE then
            if save_checkpoint(string.format("%s_scene_%s_v%d", PREFIX, label, vsync), vsync) then
                scene_seen[label] = true
            end
        end

        -- A dialogue box: page it.
        local fc = probe.read_u32(FIELD_CTL)
        local dlg = ok_ptr(fc) and (probe.read_u8(fc + 0x62) or 0) or 0
        if dlg ~= 0 and route[wi] and route[wi].talk and not route[wi].opened then
            route[wi].opened = true
            logf("vsync=%d: talk %d opened", vsync, wi)
        end
        if dlg ~= 0 and act_v == nil and not (route[wi] and route[wi].talk) then
            if vsync >= dlg_next then
                set_pad({ BTN.CROSS }); dlg_next = vsync + 20
            elseif vsync == dlg_next - 16 then
                set_pad({})
            end
            last_xy = nil
            return
        end

        -- Skip forward to the loaded scene's leg.
        while route[wi] and route[wi].scene ~= label do
            local j = wi
            while route[j] and route[j].scene ~= label do j = j + 1 end
            if route[j] then
                logf("vsync=%d: scene %s, waypoint %d -> %d", vsync, label, wi, j)
                wi = j
            else
                break
            end
        end
        local w = route[wi]
        if w == nil then
            set_pad({})
            -- The scene's own checkpoint first: the route may end before
            -- the settle window has run.
            if not scene_seen[label] then return end
            save_checkpoint(string.format("%s_end_v%d", PREFIX, vsync), vsync)
            finished = true; ctx.request_quit = true
            return
        end
        if w.scene ~= label then set_pad({}); return end

        if w.avoid then
            avoid = w.avoid
            wi = wi + 1
            return
        end
        if w.wait then
            set_pad({})
            w.w0 = w.w0 or vsync
            if vsync - w.w0 >= w.wait then wi = wi + 1; last_xy = nil end
            return
        end
        if w.talk then
            -- Open a conversation (face + Cross every 30 vsyncs), page it
            -- with Cross, and walk on once the box has stayed shut 40 vsyncs.
            if w.opened then
                -- The box byte lags the box by a page, so "shut" is read off
                -- the player instead: Cross every 20 vsyncs, and every 60 a
                -- short push toward the next waypoint; the push moving the
                -- player ends the talk.
                w.o0 = w.o0 or vsync
                local t = (vsync - w.o0) % 60
                local nx = route[wi + 1]
                if t == 40 then w.p0 = { px, pz } end
                if t >= 40 and t < 52 and nx and nx.x then
                    local oct = (probe.read_u32(PAD_OCTANT) or 0) % 8
                    set_pad(RING[((ring_dir(nx.x - px, nx.z - pz) - oct) % 8) + 1])
                elseif t == 52 then
                    set_pad({})
                    if w.p0 and (w.p0[1] ~= px or w.p0[2] ~= pz) then
                        logf("vsync=%d: talk %d done at (%d,%d)", vsync, wi, px, pz)
                        wi = wi + 1; last_xy = nil
                    end
                elseif t % 20 == 0 and t < 40 then set_pad({ BTN.CROSS })
                elseif t % 20 == 4 then set_pad({}) end
                if vsync - w.o0 > 60 * w.n * 3 then
                    save_checkpoint(string.format("%s_talk_stuck", PREFIX), vsync)
                    logf("vsync=%d: talk %d never let go", vsync, wi)
                    finished = true; ctx.request_quit = true
                end
                return
            end
            w.v0 = w.v0 or vsync
            local t = (vsync - w.v0) % 30
            local oct = (probe.read_u32(PAD_OCTANT) or 0) % 8
            if t == 0 then
                w.presses = (w.presses or 0) + 1
                if w.presses > w.n then
                    logf("vsync=%d: TALK gave up at (%d,%d)", vsync, px, pz)
                    finished = true; ctx.request_quit = true
                    return
                end
                set_pad(RING[((w.dir - oct) % 8) + 1])
            elseif t == 4 then set_pad({})
            elseif t == 10 then set_pad({ BTN.CROSS })
            elseif t == 16 then set_pad({}) end
            return
        end
        if w.act then
            act_v = act_v or vsync
            local t = (vsync - act_v) % 30
            local oct = (probe.read_u32(PAD_OCTANT) or 0) % 8
            -- Once a box is open the presses only page it: a held
            -- direction would move a picker's cursor.
            local pad_dir = (dlg ~= 0 or act_press >= ACT_FACE) and {} or RING[((w.dir - oct) % 8) + 1]
            if t == 0 then
                act_press = act_press + 1
                if ACT_SHOT_EVERY > 0 and act_press % ACT_SHOT_EVERY == 0 then
                    save_checkpoint(string.format("%s_act_p%d", PREFIX, act_press), vsync)
                end
                if act_press > w.n then
                    save_checkpoint(string.format("%s_act_gaveup", PREFIX), vsync)
                    logf("vsync=%d: ACT gave up after %d presses at (%d,%d)", vsync, w.n, px, pz)
                    finished = true; ctx.request_quit = true
                    return
                end
                set_pad(pad_dir)
            elseif t == 4 then set_pad({})
            elseif t == 10 then set_pad({ BTN.CROSS })
            elseif t == 16 then set_pad({}) end
            return
        end

        -- Teleport waypoint: a jump in position reaches it.
        if w.tp and last_xy and (math.abs(px - last_xy[1]) > 256 or math.abs(pz - last_xy[2]) > 256) then
            logf("vsync=%d: teleported (%d,%d) -> (%d,%d), waypoint %d", vsync, last_xy[1], last_xy[2], px, pz, wi)
            wi = wi + 1; last_xy = nil; still = 0
            return
        end
        local dx, dz = w.x - px, w.z - pz
        local moved = last_xy and (last_xy[1] ~= px or last_xy[2] ~= pz)
        if last_xy and not moved then still = still + 1 else still = 0 end
        last_xy = { px, pz }
        if math.abs(dx) <= TOL and math.abs(dz) <= TOL
            or (still >= 8 and math.abs(dx) <= 2 * TOL and math.abs(dz) <= 2 * TOL and not w.tp) then
            logf("vsync=%d: waypoint %d (%d,%d) reached at (%d,%d)", vsync, wi, w.x, w.z, px, pz)
            wi = wi + 1; still = 0
            if not (route[wi] and route[wi].act) then set_pad({}) end
            return
        end
        local oct = (probe.read_u32(PAD_OCTANT) or 0) % 8
        if vsync < side_until then
            if into_avoid(px, pz, side_dir) then set_pad({}); side_until = 0; return end
            set_pad(RING[((side_dir - oct) % 8) + 1])
            return
        end
        local d
        if math.abs(dx) <= TOL / 2 then d = (dz > 0) and 0 or 4
        elseif math.abs(dz) <= TOL / 2 then d = (dx > 0) and 2 or 6
        else d = ring_dir(dx, dz) end
        if still >= 10 then
            side_flip = not side_flip
            side_dir = (d + (side_flip and 2 or 6)) % 8
            if into_avoid(px, pz, side_dir) then side_dir = (side_dir + 4) % 8 end
            if into_avoid(px, pz, side_dir) then side_dir = (d + 4) % 8 end
            side_until = vsync + 16; still = 0
            logf("vsync=%d: blocked at (%d,%d) on the way to waypoint %d (%d,%d), sidestep", vsync, px, pz, wi, w.x, w.z)
            return
        end
        if into_avoid(px, pz, d) then
            -- The straight line clips a door tile: take the nearer axis.
            local alt = (math.abs(dx) > math.abs(dz)) and ((dx > 0) and 2 or 6) or ((dz > 0) and 0 or 4)
            if not into_avoid(px, pz, alt) then d = alt end
        end
        set_pad(RING[((d - oct) % 8) + 1])
        if vsync % 60 == 0 then
            logf("vsync=%d wp %d (%d,%d) player (%d,%d) oct %d", vsync, wi, w.x, w.z, px, pz, oct)
        end
    end,

    on_done = function(_, _)
        set_pad({})
        logf("done: waypoint %d of %d", wi, #route)
        local fh = io.open(probe.out_path("route.log"), "w")
        if fh ~= nil then
            fh:write(table.concat(log_lines, "\n") .. "\n")
            fh:close()
        end
    end,
})
