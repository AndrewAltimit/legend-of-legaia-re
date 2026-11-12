-- autorun_screen_walk.lua
--
-- Reach a screen on the retail disc from a field or menu state by a list of
-- steps, and save a raw state at each named stop. The capture-coverage
-- worklist's tool for a screen behind a conversation or a menu path: a shop's
-- lists, the world map's map display, a results window.
--
-- LEGAIA_STEPS is a `;` list, run in order. Every step past `poke` is pad only:
--
--   wait:<n>                 stand still n vsyncs
--   press:<BTN>[:<hold>]     tap a button (hold, default 6 vsyncs), then 40 idle
--   hold:<BTN>:<n>           hold a button n vsyncs
--   poke:<x>:<z>[:<n>]       hold the player's +0x14 / +0x18 on (x, z) for n
--                            vsyncs (default 20); a door's walk-on tile, or a
--                            spot beside an NPC. A state made with one is a
--                            poked state and its scenario must say so.
--   talk:<p90 hex>:<dist>    find the field actor whose +0x90 is p90, stand the
--                            player dist units from it, and try each facing
--                            with Cross until a box opens or the mode leaves
--                            the field
--   mash:<BTN>:<every>:mode:<hex>   press BTN every n vsyncs until the game
--                            mode is hex
--   mash:<BTN>:<every>:sub:<hex>    ... until the menu sub-screen 0x801E46A4 is hex
--   mash:<BTN>:<every>:field        ... until the field runs with no box open
--   sub:<hex>                wait for the sub-screen word to read hex
--   mode:<hex>               wait for the game mode to read hex
--   bflow:<hex>              wait for the battle flow state ctx[+0x06]
--                            (ctx = *0x8007BD24) to read hex
--   save:<name>              write <OUT_DIR>/<name>.rawsstate
--
-- A step that does not finish within LEGAIA_STEP_LIMIT (3000) vsyncs ends the
-- run with what it saved, and logs STUCK. The probe quits itself when the list
-- is done. No breakpoints: --fast and --turbo are fine.
--
-- Env: LEGAIA_SSTATE, LEGAIA_STEPS, LEGAIA_OUT_DIR, LEGAIA_FRAMES (default 20000).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 20000)
local LIMIT = probe.getenv_num("LEGAIA_STEP_LIMIT", 3000)

local GAME_MODE_VA = 0x8007b83c
local SCENE_VA = 0x8007050c
local PLAYER_PTR = 0x8007c364
local FIELD_CTL = 0x801c6ea4
local SUB_VA = 0x801e46a4
local LIST_HEADS = { 0x8007C34C, 0x8007C350, 0x8007C354, 0x8007C358, 0x8007C35C, 0x8007C360 }
local FACINGS = { "UP", "LEFT", "DOWN", "RIGHT" }

local function logf(fmt, ...) PCSX.log("[walk] " .. string.format(fmt, ...)) end
local function ok_ptr(p) return p ~= nil and p >= 0x80000000 and p < 0x80200000 end
local function s16(a)
    local v = probe.read_u16(a) or 0
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end
local function mode_now() return probe.read_u8(GAME_MODE_VA) or 0 end
local function sub_now() return probe.read_u8(SUB_VA) or 0 end
local function dialog_open()
    local c = probe.read_u32(FIELD_CTL)
    return ok_ptr(c) and (probe.read_u8(c + 0x62) or 0) ~= 0
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
local function find_actor(p90)
    for _, head in ipairs(LIST_HEADS) do
        local a = probe.read_u32(head)
        local guard = 0
        while ok_ptr(a) and guard < 256 do
            if probe.read_u32(a + 0x90) == p90 then return a end
            a = probe.read_u32(a)
            guard = guard + 1
        end
    end
end
local function put_player(x, z)
    local pp = probe.read_u32(PLAYER_PTR)
    if not ok_ptr(pp) then return end
    probe.write_u16(pp + 0x14, x % 0x10000)
    probe.write_u16(pp + 0x18, z % 0x10000)
end

local function save_state(name, vsync)
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(probe.out_path(name .. ".rawsstate"), "TRUNCATE")
        fh:writeMoveSlice(w)
        fh:close()
    end)
    logf("SAVE %s vsync=%d mode=0x%02x sub=0x%02x scene=%s ok=%s %s", name, vsync, mode_now(),
        sub_now(), scene_name(), tostring(ok), tostring(err or ""))
end

local steps = {}
for chunk in string.gmatch(probe.getenv("LEGAIA_STEPS", ""), "[^;]+") do
    local parts = {}
    for p in string.gmatch(chunk, "[^:]+") do parts[#parts + 1] = p end
    steps[#steps + 1] = parts
end

local si, st = 1, nil
local held, held_until = nil, 0
local function tap(btn, vsync, hold)
    if held then probe.pad.release(held) end
    probe.pad.force(btn)
    held, held_until = btn, vsync + (hold or 6)
end

local done = false
local function finish(ctx, why)
    logf("END %s", why)
    done = true
    ctx.request_quit = true
end

-- Each handler returns true when its step is finished.
local handlers = {
    wait = function(s, v, p)
        s.until_v = s.until_v or (v + tonumber(p[2]))
        return v >= s.until_v
    end,
    press = function(s, v, p)
        if not s.until_v then
            tap(probe.BTN[p[2]], v, tonumber(p[3] or 6))
            s.until_v = v + (tonumber(p[3] or 6)) + 40
        end
        return v >= s.until_v
    end,
    hold = function(s, v, p)
        if not s.until_v then
            tap(probe.BTN[p[2]], v, tonumber(p[3]))
            s.until_v = v + tonumber(p[3])
        end
        return v >= s.until_v
    end,
    poke = function(s, v, p)
        s.until_v = s.until_v or (v + tonumber(p[4] or 20))
        put_player(tonumber(p[2]), tonumber(p[3]))
        return v >= s.until_v
    end,
    talk = function(s, v, p)
        if mode_now() ~= 0x03 or dialog_open() then return true end
        if v < (s.next_v or 0) then return false end
        local a = find_actor(tonumber(p[2], 16))
        if not a then
            s.next_v = v + 30
            s.misses = (s.misses or 0) + 1
            return false
        end
        local x, z, d = s16(a + 0x14), s16(a + 0x18), tonumber(p[3])
        s.i = (s.i or 0) + 1
        local face = FACINGS[(s.i - 1) % 4 + 1]
        local off = ({ { 0, -d }, { d, 0 }, { 0, d }, { -d, 0 } })[(s.i - 1) % 4 + 1]
        put_player(x + off[1], z + off[2])
        logf("talk try %d npc=(%d,%d) face=%s", s.i, x, z, face)
        tap(probe.BTN[face], v, 3)
        s.cross_at = v + 8
        s.next_v = v + 40
        return false
    end,
    mash = function(s, v, p)
        local kind, val = p[4], p[5]
        if kind == "mode" and mode_now() == tonumber(val, 16) then return true end
        if kind == "sub" and sub_now() == tonumber(val, 16) then return true end
        if kind == "field" and mode_now() == 0x03 and not dialog_open() then
            s.quiet = (s.quiet or 0) + 1
            return s.quiet > 60
        end
        s.quiet = 0
        if v >= (s.next_v or 0) then
            tap(probe.BTN[p[2]], v, 4)
            s.next_v = v + tonumber(p[3])
        end
        return false
    end,
    sub = function(s, v, p) return sub_now() == tonumber(p[2], 16) end,
    mode = function(s, v, p) return mode_now() == tonumber(p[2], 16) end,
    bflow = function(s, v, p)
        local c = probe.read_u32(0x8007BD24)
        return ok_ptr(c) and (probe.read_u8(c + 0x06) or 0) == tonumber(p[2], 16)
    end,
    save = function(s, v, p)
        save_state(p[2], v)
        return true
    end,
}

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function(ctx)
        logf("sstate=%s steps=%d", SSTATE_PATH, #steps)
        return {}
    end,
    on_capture = function(ctx, vsync)
        if done then return end
        if held and vsync >= held_until then
            probe.pad.release(held)
            held = nil
        end
        if st and st.cross_at and vsync >= st.cross_at then
            st.cross_at = nil
            tap(probe.BTN.CROSS, vsync, 4)
        end
        if si > #steps then return finish(ctx, "steps done") end
        if not st then
            st = { began = vsync }
            logf("step %d %s at vsync=%d mode=0x%02x sub=0x%02x scene=%s", si,
                table.concat(steps[si], ":"), vsync, mode_now(), sub_now(), scene_name())
        end
        local p = steps[si]
        if handlers[p[1]](st, vsync, p) then
            si, st = si + 1, nil
        elseif vsync - st.began > LIMIT then
            return finish(ctx, "STUCK on step " .. table.concat(p, ":"))
        end
    end,
})
