-- autorun_w5a_poke_walk.lua
--
-- Cross a scripted route of scene doors without a pad ladder.
--
-- A walk-on door is a `.MAP` kind-1 trigger: the per-frame tile check
-- (`FUN_801D1EC4`) matches the player's tile EXACTLY and spawns the named
-- partition-2 record, whose `0x3F` does the scene change. So a probe that
-- writes the player object's `+0x14`/`+0x18` to a trigger tile crosses the
-- door on the next frame, and the tiles come off the disc
-- (`legaia_engine_core::field_regions::parse_tile_triggers` over the
-- `.MAP` `+0x10000` primary and `+0x12000` fallback blocks) rather than
-- out of a hand-calibrated input sequence. Two scenes deep, a pad ladder
-- costs an hour of wall clock and breaks on any encounter; this costs one
-- run.
--
-- What it is NOT: a locomotion test. The player is teleported, so nothing
-- it produces says anything about walking, collision or the camera's
-- follow - only about what the ARRIVAL scene does. For a route whose
-- point is the walk itself, use `autorun_w3a_field_walk.lua`.
--
-- Route: LEGAIA_ROUTE = "<scene>@<tileX>,<tileZ>;<scene>@<tileX>,<tileZ>"
-- - each leg waits for field mode in its own scene, settles
-- LEGAIA_SETTLE vsyncs, then pokes the tile every vsync until the scene
-- name changes. A leg naming a scene the route never reaches simply never
-- fires, and the log says which leg it stopped on.
--
-- Env:
--   LEGAIA_ROUTE        the legs, as above (required to move)
--   LEGAIA_SETTLE       vsyncs of field mode before a leg pokes (default 90)
--   LEGAIA_CKPT_SCENE   write `<label>.rawsstate` on reaching this scene
--   LEGAIA_CKPT_LABEL   checkpoint stem (default "poke_walk")
--   LEGAIA_FRAMES       vsyncs to keep running after the last leg lands
--   LEGAIA_TINT         1 = also log `FUN_80024EE4(bucket, abr, colour)`
--   LEGAIA_PRESS        "frame:BTN:dur,..." absolute-vsync pad presses,
--                       for an interaction the route cannot reach by tile
--   LEGAIA_MASH         "<BTN>:<period>:<dur>" - press BTN every `period`
--                       vsyncs once the LAST leg has poked. A door record
--                       may open a Yes/No picker before its `0x3F` (the
--                       conc2 vortex asks before it warps), and a tile poke
--                       alone parks the script on the picker forever
--
-- Output: w5a_route.csv (per vsync), w5a_route_hits.csv (tint pushes),
-- w5a_route.log.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local bp    = require("probe.bp")
local pad   = require("probe.pad")
local bit   = require("bit")

local GAME_MODE  = 0x8007B83C
local SCENE_NAME = 0x8007050C
local PLAYER_PTR = 0x8007C364
local TINT_R     = 0x8007BCB8
local PUSH_QUAD  = 0x80024EE4

local SSTATE     = probe.getenv("LEGAIA_SSTATE", "")
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local SETTLE     = probe.getenv_num("LEGAIA_SETTLE", 90)
local POST       = probe.getenv_num("LEGAIA_FRAMES", 300)
local MAX_TICKS  = probe.getenv_num("LEGAIA_MAX_TICKS", 4000)
local CKPT_SCENE = probe.getenv("LEGAIA_CKPT_SCENE", "")
local CKPT_LABEL = probe.getenv("LEGAIA_CKPT_LABEL", "poke_walk")
local WANT_TINT  = probe.getenv("LEGAIA_TINT", "") == "1"
local OUT_DIR    = probe.getenv("LEGAIA_OUT_DIR", "captures/w5a_poke_walk")

local BTN = {
    up = pad.BTN.UP, down = pad.BTN.DOWN, left = pad.BTN.LEFT,
    right = pad.BTN.RIGHT, cross = pad.BTN.CROSS, circle = pad.BTN.CIRCLE,
    triangle = pad.BTN.TRIANGLE, square = pad.BTN.SQUARE,
    start = pad.BTN.START, select = pad.BTN.SELECT,
}

local mash = nil
do
    local name, period, dur = string.match(probe.getenv("LEGAIA_MASH", ""), "^(%a+):(%d+):(%d+)$")
    if name then mash = { name = name, period = tonumber(period), dur = tonumber(dur) } end
end

local route = {}
for tok in string.gmatch(probe.getenv("LEGAIA_ROUTE", ""), "[^;%s]+") do
    local scene, tx, tz = string.match(tok, "^(%w+)@(%d+),(%d+)$")
    if scene == nil then error("LEGAIA_ROUTE leg '" .. tok .. "' is not <scene>@<x>,<z>") end
    route[#route + 1] = { scene = scene, tx = tonumber(tx), tz = tonumber(tz) }
end

local presses = {}
for tok in string.gmatch(probe.getenv("LEGAIA_PRESS", ""), "[^,%s]+") do
    local at, name, dur = string.match(tok, "^(%d+):(%a+):(%d+)$")
    local b = name and BTN[string.lower(name)]
    if b == nil then error("LEGAIA_PRESS step '" .. tok .. "' is not <frame>:<button>:<for>") end
    presses[#presses + 1] = { at = tonumber(at), dur = tonumber(dur), btn = b, name = name }
end

local CSV = probe.csv_open(probe.out_path("w5a_route.csv"),
    "vsync,scene,mode,px,pz,tile_x,tile_z,leg,tint_r,tint_g,tint_b")
local HITS = probe.csv_open(probe.out_path("w5a_route_hits.csv"),
    "vsync,site,a0,a1,a2,ra,scene,mode")
local LOGF = io.open(probe.out_path("w5a_route.log"), "w")

local function log(s)
    PCSX.log("[w5a_route] " .. s)
    if LOGF then LOGF:write(s .. "\n"); LOGF:flush() end
end

local function u8(a) return mem.read_u8(a) or 0 end
local function u32n(v) v = tonumber(v) or 0; if v < 0 then v = v + 4294967296 end; return v end
local function hex32(v) return string.format("0x%08X", u32n(v)) end

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

local function player_xz()
    local p = player_ptr()
    if p == nil then return -1, -1 end
    local x = (mem.read_u16(p + 0x14) or 0) % 0x10000
    local z = (mem.read_u16(p + 0x18) or 0) % 0x10000
    if x >= 0x8000 then x = x - 0x10000 end
    if z >= 0x8000 then z = z - 0x10000 end
    return x, z
end

-- Retail's own tile quantisation for the trigger compare: `(world - 0x40) >> 7`.
local function tile_of(v) return math.floor((v - 0x40) / 128) end
local function world_of(t) return t * 128 + 0x40 end

local vsync, leg, field_ticks, post_ticks = 0, 1, 0, 0
local mash_from = nil
local loaded_at, armed, done = nil, false, false
local last_key, ckpt_done = nil, false
local held = {}

local function checkpoint()
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(OUT_DIR .. "/" .. CKPT_LABEL .. ".rawsstate", "CREATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    log("checkpoint " .. tostring(ok) .. " " .. tostring(err))
end

local function finish(why)
    if done then return end
    done = true
    log(string.format("%s at tick %d; leg %d/%d scene=%s mode=0x%02X",
        why, vsync, leg, #route, scene_name(), u8(GAME_MODE)))
    pcall(function() bp.disarm() end)
    CSV:close(); HITS:close()
    if LOGF then LOGF:close() end
    PCSX.quit(0)
end

local function arm_all()
    if WANT_TINT then
        bp.arm(PUSH_QUAD, "Exec", 4, "push_quad", function()
            local r = PCSX.getRegisters()
            HITS:row("%d,push_80024EE4,%d,%d,%s,%s,%s,0x%02X", vsync,
                u32n(r.GPR.n.a0), u32n(r.GPR.n.a1), hex32(r.GPR.n.a2),
                hex32(r.GPR.n.ra), scene_name(), u8(GAME_MODE))
            HITS.fh:flush()
        end)
    end
    armed = true
    log(string.format("armed; %d leg(s), settle=%d, tint=%s",
        #route, SETTLE, tostring(WANT_TINT)))
    for i, l in ipairs(route) do
        log(string.format("  leg %d: %s @ tile (%d,%d) -> world (%d,%d)",
            i, l.scene, l.tx, l.tz, world_of(l.tx), world_of(l.tz)))
    end
end

local function on_vsync()
    if done then return end
    vsync = vsync + 1

    if loaded_at == nil then
        if SSTATE == "" then
            loaded_at = vsync
        elseif vsync >= BOOT_DELAY then
            if not probe.load_save_state(SSTATE) then
                log("FATAL: could not load " .. SSTATE); finish("load failed"); return
            end
            loaded_at = vsync
            log(string.format("state loaded at tick %d; scene=%s mode=0x%02X",
                vsync, scene_name(), u8(GAME_MODE)))
        end
        return
    end
    if not armed then arm_all(); return end

    for i, p in ipairs(presses) do
        if vsync == p.at then pad.force(p.btn); held[i] = true; log("press " .. p.name .. " @" .. vsync)
        elseif vsync == p.at + p.dur then pad.release(p.btn); held[i] = nil end
    end

    local sc, md = scene_name(), u8(GAME_MODE)
    local px, pz = player_xz()
    CSV:row("%d,%s,0x%02X,%d,%d,%d,%d,%d,%d,%d,%d", vsync, sc, md, px, pz,
        tile_of(px), tile_of(pz), leg, u8(TINT_R), u8(TINT_R + 1), u8(TINT_R + 2))
    local key = sc .. "|" .. md .. "|" .. leg
    if key ~= last_key then
        last_key = key
        log(string.format("f=%d scene=%s mode=0x%02X player=(%d,%d) tile=(%d,%d) leg=%d",
            vsync, sc, md, px, pz, tile_of(px), tile_of(pz), leg))
    end

    if md == 0x03 then field_ticks = field_ticks + 1 else field_ticks = 0 end

    local l = route[leg]
    if l ~= nil and md == 0x03 and sc == l.scene and field_ticks >= SETTLE then
        local p = player_ptr()
        if p ~= nil then
            mem.write_u16(p + 0x14, world_of(l.tx) % 0x10000)
            mem.write_u16(p + 0x18, world_of(l.tz) % 0x10000)
        end
        if mash ~= nil and mash_from == nil and leg == #route then
            mash_from = vsync
            log("mash " .. mash.name .. " armed from tick " .. vsync)
        end
    end

    -- Confirm whatever picker the last leg's door record opens.
    if mash ~= nil and mash_from ~= nil then
        local btn = BTN[string.lower(mash.name)]
        local phase = (vsync - mash_from) % mash.period
        if btn ~= nil then
            if phase == 0 then pad.force(btn)
            elseif phase == mash.dur then pad.release(btn) end
        end
    end
    -- Advance the leg the moment the scene name leaves the leg's scene.
    if l ~= nil and sc ~= "" and sc ~= l.scene and field_ticks >= 2 then
        log(string.format("leg %d crossed: now in %s at tick %d", leg, sc, vsync))
        leg = leg + 1
        field_ticks = 0
    end

    if CKPT_SCENE ~= "" and not ckpt_done and sc == CKPT_SCENE and md == 0x03 and field_ticks >= 40 then
        ckpt_done = true
        log("reached checkpoint scene " .. sc .. " at tick " .. vsync)
        checkpoint()
    end

    if leg > #route and md == 0x03 then
        post_ticks = post_ticks + 1
        if post_ticks >= POST then finish("route complete") end
    end
    if vsync >= MAX_TICKS then finish("max ticks") end
end

os.execute(string.format("mkdir -p %q", OUT_DIR))
log("=== autorun_w5a_poke_walk ===")
log(string.format("sstate=%s route=%d legs ckpt=%s",
    SSTATE == "" and "(none)" or SSTATE, #route,
    CKPT_SCENE == "" and "(none)" or CKPT_SCENE))

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
