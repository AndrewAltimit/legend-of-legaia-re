-- autorun_bio_castle_entrance.lua
--
-- How does retail let the party into the Bio Castle from the Drake
-- overworld?
--
-- Once story flag 0x3BA is up, `map01`'s system script (P1[0]) paints the
-- castle's walls into the collision grid with four `4C 7x` ops, object
-- P0[8] (Rim Elm's landmark) moves to tile (96, 25) and wears the castle
-- mesh, and the walk-on trigger at tile (95, 23) runs P2[37], whose `0x3F`
-- enters `jou`. This probe reaches `map01` with the flag up the cheap way
-- (a tile-poke route out of the Bio Castle from `jouind_field_card_boot`,
-- the PRO-10 card save), then:
--
--   1. dumps the live collision grid's wall nibbles around the door
--      (`*(0x1F8003EC) + 0x4000`, rows of 0x80 bytes) to `grid.txt`;
--   2. screenshots the arrival frame (`arrival.raw`);
--   3. runs pad walks from outside the castle, one per start tile: the
--      player is poked onto the start tile's centre, then a direction is
--      held. Each walk logs the player every vsync to `walks.csv` and ends
--      on a scene change or after `LEGAIA_WALK_FOR` vsyncs.
--
-- The walks are a locomotion test from a poked seat: the seat is a poke,
-- every step after it is the retail controller on a held pad.
--
-- Env:
--   LEGAIA_ROUTE      tile-poke legs out to map01 (see autorun_w5a_poke_walk)
--   LEGAIA_WALKS      "x,z,BTN[+BTN];..." start tiles + held buttons, in order
--   LEGAIA_WALK_FOR   vsyncs per walk (default 300)
--   LEGAIA_SETTLE     vsyncs of field mode before a leg / walk (default 90)
--   LEGAIA_OUT_DIR    output directory

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local pad   = require("probe.pad")

local GAME_MODE  = 0x8007B83C
local SCENE_NAME = 0x8007050C
local PLAYER_PTR = 0x8007C364
local MAP_PTR    = 0x1F8003EC

local SSTATE     = probe.getenv("LEGAIA_SSTATE", "")
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local SETTLE     = probe.getenv_num("LEGAIA_SETTLE", 90)
local WALK_FOR   = probe.getenv_num("LEGAIA_WALK_FOR", 300)
local MAX_TICKS  = probe.getenv_num("LEGAIA_MAX_TICKS", 12000)
local OUT_DIR    = probe.getenv("LEGAIA_OUT_DIR", "captures/bio_castle_entrance")

local BTN = {
    up = pad.BTN.UP, down = pad.BTN.DOWN, left = pad.BTN.LEFT, right = pad.BTN.RIGHT,
}

local route = {}
for tok in string.gmatch(probe.getenv("LEGAIA_ROUTE", ""), "[^;%s]+") do
    local scene, tx, tz = string.match(tok, "^(%w+)@(%d+),(%d+)$")
    route[#route + 1] = { scene = scene, tx = tonumber(tx), tz = tonumber(tz) }
end
local walks = {}
for tok in string.gmatch(probe.getenv("LEGAIA_WALKS", ""), "[^;%s]+") do
    local tx, tz, b = string.match(tok, "^(%d+),(%d+),([%a+]+)$")
    local btns = {}
    for name in string.gmatch(b, "%a+") do btns[#btns + 1] = BTN[string.lower(name)] end
    walks[#walks + 1] = { tx = tonumber(tx), tz = tonumber(tz), btns = btns, name = b }
end

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOGF = io.open(OUT_DIR .. "/bio.log", "w")
local WCSV = io.open(OUT_DIR .. "/walks.csv", "w")
WCSV:write("walk,vsync,scene,mode,px,pz,tile_x,tile_z\n")
local function log(s)
    PCSX.log("[bio] " .. s)
    LOGF:write(s .. "\n"); LOGF:flush()
end

local function u8(a) return mem.read_u8(a) or 0 end
local function u32n(v) v = tonumber(v) or 0; if v < 0 then v = v + 4294967296 end; return v end
local function scene_name()
    local s = {}
    for i = 0, 7 do
        local b = u8(SCENE_NAME + i)
        if b < 0x20 or b >= 0x7F then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end
local function player_ptr()
    local p = u32n(mem.read_u32(PLAYER_PTR) or 0)
    if p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end
local function s16(v) v = (v or 0) % 0x10000; if v >= 0x8000 then v = v - 0x10000 end; return v end
local function player_xz()
    local p = player_ptr()
    if p == nil then return -1, -1 end
    return s16(mem.read_u16(p + 0x14)), s16(mem.read_u16(p + 0x18))
end
local function world_of(t) return t * 128 + 0x40 end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then log("screenshot failed"); return end
    local fh = io.open(OUT_DIR .. "/" .. stem .. ".raw", "wb")
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(OUT_DIR .. "/" .. stem .. ".raw.meta", "w")
    mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n", ss.width or 320,
        ss.height or 240, (ss.bpp == "BPP_24") and 24 or 16))
    mh:close()
    log("screenshot " .. stem)
end

-- The wall nibbles of tiles x 86..104, z 16..32, as the grid stores them:
-- one hex digit per tile (the byte's high nibble, its four sub-cell bits).
local function dump_grid()
    local base = u32n(mem.read_scratch_u32(MAP_PTR) or 0)
    local fh = io.open(OUT_DIR .. "/grid.txt", "w")
    fh:write(string.format("map=0x%08X grid=+0x4000, high nibble per tile, cols 86..104\n", base))
    for row = 16, 32 do
        local line = {}
        for col = 86, 104 do
            local b = u8(base + 0x4000 + row * 0x80 + col)
            line[#line + 1] = string.format("%X", math.floor(b / 16))
        end
        fh:write(string.format("row %3d  %s\n", row, table.concat(line, " ")))
    end
    fh:close()
    log(string.format("grid dumped (map 0x%08X)", base))
end

local vsync, leg, field_ticks = 0, 1, 0
local loaded_at, phase = nil, "route"
local walk_i, walk_t = 0, 0
local done = false

local function finish(why)
    if done then return end
    done = true
    log(string.format("%s at tick %d scene=%s", why, vsync, scene_name()))
    WCSV:close(); LOGF:close()
    PCSX.quit(0)
end

local function on_vsync()
    if done then return end
    vsync = vsync + 1
    if loaded_at == nil then
        if SSTATE == "" then loaded_at = vsync
        elseif vsync >= BOOT_DELAY then
            if not probe.load_save_state(SSTATE) then finish("load failed"); return end
            loaded_at = vsync
            log("state loaded; scene=" .. scene_name())
        end
        return
    end
    local sc, md = scene_name(), u8(GAME_MODE)
    if md == 0x03 then field_ticks = field_ticks + 1 else field_ticks = 0 end

    if phase == "route" then
        local l = route[leg]
        if l == nil then
            if sc == "map01" and md == 0x03 and field_ticks >= SETTLE then
                local px, pz = player_xz()
                log(string.format("on map01 at (%d,%d)", px, pz))
                dump_grid()
                screenshot("arrival")
                phase = "walk"
                walk_i, walk_t = 1, 0
            end
        else
            if md == 0x03 and sc == l.scene and field_ticks >= SETTLE then
                local p = player_ptr()
                if p then
                    mem.write_u16(p + 0x14, world_of(l.tx) % 0x10000)
                    mem.write_u16(p + 0x18, world_of(l.tz) % 0x10000)
                end
            end
            if sc ~= "" and sc ~= l.scene and field_ticks >= 2 then
                log(string.format("leg %d crossed into %s at %d", leg, sc, vsync))
                leg = leg + 1
                field_ticks = 0
            end
        end
    elseif phase == "walk" then
        local w = walks[walk_i]
        if w == nil then finish("walks complete"); return end
        if sc ~= "map01" then
            log(string.format("walk %d (%d,%d %s) entered %s at t=%d", walk_i, w.tx, w.tz,
                w.name, sc, walk_t))
            for _, b in ipairs(w.btns) do pad.release(b) end
            finish("scene change")
            return
        end
        walk_t = walk_t + 1
        if walk_t == 1 then
            local p = player_ptr()
            if p then
                mem.write_u16(p + 0x14, world_of(w.tx) % 0x10000)
                mem.write_u16(p + 0x18, world_of(w.tz) % 0x10000)
            end
        elseif walk_t == 30 then
            for _, b in ipairs(w.btns) do pad.force(b) end
        elseif walk_t == 30 + WALK_FOR then
            for _, b in ipairs(w.btns) do pad.release(b) end
            local px, pz = player_xz()
            log(string.format("walk %d (%d,%d %s): stopped at (%d,%d) tile (%d,%d)", walk_i,
                w.tx, w.tz, w.name, px, pz, math.floor(px / 128), math.floor(pz / 128)))
            if walk_i == 1 then screenshot("walk1_end") end
        elseif walk_t >= 30 + WALK_FOR + 20 then
            walk_i, walk_t = walk_i + 1, 0
        end
        local px, pz = player_xz()
        WCSV:write(string.format("%d,%d,%s,0x%02X,%d,%d,%d,%d\n", walk_i, vsync, sc, md, px, pz,
            math.floor(px / 128), math.floor(pz / 128)))
    end
    if vsync >= MAX_TICKS then finish("max ticks") end
end

log("=== autorun_bio_castle_entrance ===")
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
