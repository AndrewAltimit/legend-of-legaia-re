-- autorun_seat_walk_film.lua
--
-- Film a stretch of a field scene the library holds no state in: load a state
-- anywhere in the scene, seat the player on a tile, hold a pad schedule, and
-- write the displayed frame every few vsyncs beside the player, the follow
-- camera and the running record. The seat is a position poke (player +0x14 /
-- +0x18, held for a few vsyncs so the walk-on tests see it), so what it films
-- is the scene's own geometry, camera zones, walk-on records and draw, with
-- whatever party and story flags the source state carried. A film made this
-- way is a poked capture: say so wherever it is cited.
--
-- Written for the Mt. Rikuroa cave (`rikuroa`, the stretch Noa walks alone):
-- the rockfall records P2[44] / P2[45] test only their own flags, so a state
-- standing at the mountain's foot reaches them through the cave mouth's
-- teleport edge.
--
-- Env:
--   LEGAIA_SSTATE      source state (or --scenario on the runner)
--   LEGAIA_FRAMES      vsyncs to film (default 1200)
--   LEGAIA_SEATS       `;` list of `<vsync>:<tile x>:<tile z>`: at that vsync
--                      write the tile's centre, held LEGAIA_SEAT_FOR (8) vsyncs
--   LEGAIA_PAD_SCRIPT  `A-B:BTN` spans in vsyncs from the load, comma list
--   LEGAIA_SHOT_EVERY  vsyncs between frames (default 10; 0 = none)
--   LEGAIA_SHOT_FROM   first vsync to shoot (default 0)
--   LEGAIA_FLAGS_SET / LEGAIA_FLAGS_CLEAR
--                      comma lists of story flags (hex, the 0x80085758 bank)
--                      written once as the run starts
--   LEGAIA_CKPT        `;` list of `<vsync>:<name>`: save a raw state there
--
-- Outputs: film.csv (one row a vsync), shot_<vsync>.raw (+ .raw.meta) frames,
-- <name>.rawsstate checkpoints. Poll-only, safe under --fast.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1200)
local SCRIPT = probe.getenv("LEGAIA_PAD_SCRIPT", "")
local SHOT_EVERY = probe.getenv_num("LEGAIA_SHOT_EVERY", 10)
local SHOT_FROM = probe.getenv_num("LEGAIA_SHOT_FROM", 0)
local SEAT_FOR = probe.getenv_num("LEGAIA_SEAT_FOR", 8)

local PLAYER_PTR = 0x8007C364
local PITCH, YAW = 0x8007B790, 0x8007B792
local H          = 0x8007B6F4
local EYE        = 0x800840B8
local FOCUS_X    = 0x80089118
local FOCUS_Z    = 0x80089120
local GAME_MODE  = 0x8007B83C
local FLAG_BANK  = 0x80085758
local FIELD_CTL  = 0x801C6EA4
local VIEW_WIN   = 0x1F8003E8

local function s16(a)
    local v = probe.read_u16(a) or 0
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end
local function s32(a)
    local v = probe.read_u32(a) or 0
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end

local spans = {}
for item in string.gmatch(SCRIPT, "[^,]+") do
    local a, b, name = string.match(item, "^%s*(%d+)-(%d+):(%a+%d?)%s*$")
    local btn = name and probe.BTN[string.upper(name)]
    if a and btn then
        spans[#spans + 1] = { from = tonumber(a), to = tonumber(b), btn = btn }
    else
        PCSX.log("[seat-film] ignoring pad item '" .. item .. "'")
    end
end

local seats = {}
for v, x, z in string.gmatch(probe.getenv("LEGAIA_SEATS", ""), "(%d+):(%d+):(%d+)") do
    seats[#seats + 1] = { at = tonumber(v), x = tonumber(x) * 128 + 64, z = tonumber(z) * 128 + 64 }
end

local ckpts = {}
for v, name in string.gmatch(probe.getenv("LEGAIA_CKPT", ""), "(%d+):([%w_%-]+)") do
    ckpts[#ckpts + 1] = { at = tonumber(v), name = name }
end

local function flag_list(name)
    local out = {}
    for f in string.gmatch(probe.getenv(name, ""), "[%x]+") do
        out[#out + 1] = tonumber(f, 16)
    end
    return out
end
local flags_set, flags_clear = flag_list("LEGAIA_FLAGS_SET"), flag_list("LEGAIA_FLAGS_CLEAR")

local function shot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 240,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
end

local function save_checkpoint(name)
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(probe.out_path(name .. ".rawsstate"), "TRUNCATE")
        fh:writeMoveSlice(w)
        fh:close()
    end)
    PCSX.log(string.format("[seat-film] CKPT %s ok=%s %s", name, tostring(ok), tostring(err or "")))
end

local held = {}
local fh = assert(io.open(probe.out_path("film.csv"), "w"))
fh:write("vsync,x,y,z,heading,clip,pitch,yaw,h,eye_x,eye_y,eye_z,focus_x,focus_z," ..
    "mode,dialog,win_x0,win_z0,win_x1,win_z1,f281,f282\n")

local function flag(f)
    local b = probe.read_u8(FLAG_BANK + bit.rshift(f, 3)) or 0
    return (bit.band(b, bit.rshift(0x80, bit.band(f, 7))) ~= 0) and 1 or 0
end

local flags_done = false

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_seat_walk_film.lua",
            { sstate = SSTATE, frames = FRAMES, pad = SCRIPT,
              seats = probe.getenv("LEGAIA_SEATS", "") })
        return {}
    end,
    on_capture = function(_, el)
        if not flags_done then
            flags_done = true
            for _, f in ipairs(flags_set) do
                local va = FLAG_BANK + bit.rshift(f, 3)
                probe.write_u8(va, bit.bor(probe.read_u8(va) or 0, bit.rshift(0x80, bit.band(f, 7))))
            end
            for _, f in ipairs(flags_clear) do
                local va = FLAG_BANK + bit.rshift(f, 3)
                probe.write_u8(va, bit.band(probe.read_u8(va) or 0, 0xFF - bit.rshift(0x80, bit.band(f, 7))))
            end
        end
        local want = {}
        for _, s in ipairs(spans) do
            if el >= s.from and el < s.to then want[s.btn] = true end
        end
        for btn, _ in pairs(held) do
            if not want[btn] then probe.pad_release(btn); held[btn] = nil end
        end
        for btn, _ in pairs(want) do
            if not held[btn] then probe.pad_force(btn); held[btn] = true end
        end
        local p = probe.read_u32(PLAYER_PTR)
        if p == nil or not probe.in_ram(p) then return end
        for _, s in ipairs(seats) do
            if el >= s.at and el < s.at + SEAT_FOR then
                probe.write_u16(p + 0x14, s.x)
                probe.write_u16(p + 0x18, s.z)
            end
        end
        for _, c in ipairs(ckpts) do
            if el == c.at then save_checkpoint(c.name) end
        end
        local ctl = probe.read_u32(FIELD_CTL) or 0
        local dialog = probe.in_ram(ctl) and (probe.read_u8(ctl + 0x62) or 0) or 0
        fh:write(string.format("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d\n",
            el, s16(p + 0x14), s16(p + 0x16), s16(p + 0x18), s16(p + 0x26), s16(p + 0x5C),
            s16(PITCH), s16(YAW), s16(H),
            s32(EYE), s32(EYE + 4), s32(EYE + 8), s32(FOCUS_X), s32(FOCUS_Z),
            (probe.read_u32(GAME_MODE) or 0) % 256, dialog,
            probe.read_u8(VIEW_WIN) or 0, probe.read_u8(VIEW_WIN + 1) or 0,
            probe.read_u8(VIEW_WIN + 2) or 0, probe.read_u8(VIEW_WIN + 3) or 0,
            flag(0x281), flag(0x282)))
        fh:flush()
        if SHOT_EVERY > 0 and el >= SHOT_FROM and (el - SHOT_FROM) % SHOT_EVERY == 0 then
            shot(string.format("shot_%05d", el))
        end
    end,
})
