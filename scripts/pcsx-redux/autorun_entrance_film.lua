-- autorun_entrance_film.lua
--
-- One retail walk into an overworld entrance, filmed a vsync at a time: load a
-- state standing a step short of an entrance tile, hold a pad schedule, and
-- write the player's seat and engaged bit, the scene label, the game mode, the
-- scene-change actor's countdown and screen byte, the effect-tween slot and
-- the displayed frame's mean brightness. Answers "when does the player stop,
-- when does the fade start, when does the scene load" against the port's
-- timeline (docs/subsystems/world-map.md, "Auto-engage on walk-over").
-- Poll-only, safe under --fast.
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES (default 200), LEGAIA_OUT_DIR,
--      LEGAIA_PAD_SCRIPT  `A-B:BTN` spans in vsyncs from the load
--      (default `0-120:DOWN`).
--
-- Output: entrance_film.csv, one row a vsync.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 200)
local SCRIPT = probe.getenv("LEGAIA_PAD_SCRIPT", "0-120:DOWN")

local PLAYER_PTR = 0x8007C364
local SCENE_VA = 0x8007050C
local GAME_MODE = 0x8007B83C
local COUNTDOWN = 0x8007BA28 -- gp+0x710, the scene-change actor's countdown
local SCREEN_BYTE = 0x8007B648 -- the scene-change actor's state-0 store
local TWEEN_SLOT = 0x8007B62C -- the op-0x34 sub-0 colour tween's actor

local function s16(a)
    local v = probe.read_u16(a)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end
local function s32(a)
    local v = probe.read_u32(a)
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end

local function scene_label()
    local out = {}
    for i = 0, 11 do
        local b = probe.read_u8(SCENE_VA + i)
        if b == 0 then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

-- Mean of a 32 x 24 grid of the displayed frame's pixels (0..255 per
-- channel, averaged over R, G and B). -1 when the shot is unavailable.
local function brightness()
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return -1 end
    local data = tostring(ss.data)
    local w, h = ss.width or 320, ss.height or 240
    local bpp24 = ss.bpp == "BPP_24"
    local sum, n = 0, 0
    for gy = 0, 23 do
        local y = math.floor((gy + 0.5) * h / 24)
        for gx = 0, 31 do
            local x = math.floor((gx + 0.5) * w / 32)
            if bpp24 then
                local o = (y * w + x) * 3 + 1
                local r, g, b = string.byte(data, o, o + 2)
                if r then sum = sum + r + g + b; n = n + 3 end
            else
                local o = (y * w + x) * 2 + 1
                local lo, hi = string.byte(data, o, o + 1)
                if lo then
                    local v = lo + hi * 256
                    local r, g, b = v % 32, math.floor(v / 32) % 32, math.floor(v / 1024) % 32
                    sum = sum + (r + g + b) * 255 / 31
                    n = n + 3
                end
            end
        end
    end
    if n == 0 then return -1 end
    return math.floor(sum / n + 0.5)
end

local spans = {}
for item in string.gmatch(SCRIPT, "[^,]+") do
    local a, b, name = string.match(item, "^%s*(%d+)-(%d+):(%a+)%s*$")
    local btn = name and probe.BTN[string.upper(name)]
    if a and btn then
        spans[#spans + 1] = { from = tonumber(a), to = tonumber(b), btn = btn }
    end
end

local held = {}
local fh = assert(io.open(probe.out_path("entrance_film.csv"), "w"))
fh:write("vsync,pad,scene,mode,x,z,tile_x,tile_z,flags,engaged,countdown,screen_byte,tween,bright\n")

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_entrance_film.lua",
            { sstate = SSTATE, frames = FRAMES, pad = SCRIPT })
        return {}
    end,
    on_capture = function(_, el)
        local want, names = {}, {}
        for _, s in ipairs(spans) do
            if el >= s.from and el < s.to then want[s.btn] = true end
        end
        for btn, _ in pairs(held) do
            if not want[btn] then probe.pad_release(btn); held[btn] = nil end
        end
        for btn, _ in pairs(want) do
            if not held[btn] then probe.pad_force(btn); held[btn] = true end
            names[#names + 1] = tostring(btn)
        end
        local p = probe.read_u32(PLAYER_PTR)
        local x, z, flags = 0, 0, 0
        if p ~= nil and probe.in_ram(p) then
            x, z, flags = s16(p + 0x14), s16(p + 0x18), probe.read_u32(p + 0x10)
        end
        fh:write(string.format("%d,%s,%s,%d,%d,%d,%d,%d,%08X,%d,%d,%d,%08X,%d\n",
            el, table.concat(names, "+"), scene_label(), probe.read_u16(GAME_MODE),
            x, z, math.floor(x / 128), math.floor(z / 128), flags,
            (flags % 0x100000 >= 0x80000) and 1 or 0,
            s32(COUNTDOWN), probe.read_u8(SCREEN_BYTE), probe.read_u32(TWEEN_SLOT),
            brightness()))
        fh:flush()
    end,
})
