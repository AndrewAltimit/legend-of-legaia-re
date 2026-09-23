-- autorun_w6c_spoke_walk.lua
--
-- Tile-poke route (the `autorun_w5a_poke_walk.lua` mechanism) with a
-- story-flag FIREHOSE armed across the whole run, plus optional flag pokes.
--
-- Built for the region story-flag spokes: load a card-boot field anchor,
-- cross the doors a spoke needs by writing the player object's
-- `+0x14`/`+0x18` onto `.MAP` kind-1 trigger tiles, and log every system-flag
-- SET / CLEAR the run issues with the writer's `ra`, the scene it happened
-- in and the field VM's pc offset (`s8`) plus the op bytes (at `s0`), so a
-- family's play order
-- reads straight off the CSV.
--
-- The firehose is two exec-bps on the bank helpers - SET `FUN_8003CE08`,
-- CLEAR `FUN_8003CE34` (a0 = flag index) - the same pair
-- `autorun_w5a_flag_watch.lua` filters to one flag; here nothing is
-- filtered, and `w6c_spoke_flags_first.csv` keeps the first hit per
-- (op, flag, ra) so a per-frame selector's traffic cannot bury a one-shot
-- latch.
--
-- Route legs: `LEGAIA_ROUTE = "<scene>@<x>,<z>[!];..."`. A plain leg pokes
-- the tile every vsync until the scene name changes (a door). A leg ending
-- in `!` is a STAY leg: it pokes for LEGAIA_POKE_FOR vsyncs then advances
-- without waiting for a scene change - for a walk-on record that sets
-- flags or plays a beat but does not warp. The next leg then waits a fresh
-- LEGAIA_SETTLE before it pokes, so the beat plays out first. A stay leg
-- written `!<flaghex>` (e.g. `kor5@32,43!43A`) instead stops poking and
-- waits for that flag to read set before advancing - for a beat whose
-- length is measured in text boxes rather than vsyncs.
--
-- Env (on top of the poke-walk's LEGAIA_SETTLE / LEGAIA_FRAMES /
-- LEGAIA_MAX_TICKS / LEGAIA_TINT / LEGAIA_PRESS / LEGAIA_MASH):
--   LEGAIA_POKE_FOR     vsyncs a `!` leg pokes (default 20)
--   LEGAIA_MASH_ALWAYS  1 = LEGAIA_MASH runs from arming, not from the last
--                       leg (text boxes in a walk-on beat park on Cross)
--   LEGAIA_FLAG_POKE    "<scene>:<flag>=<0|1>,..." - written straight into
--                       the bank byte (`0x80085758 + (flag >> 3)`, MSB-first)
--                       once, the first field vsync in <scene>. A synthetic
--                       gate: say so wherever a result rests on it.
--   LEGAIA_MIRROR       1 = also log the `4C 86` reflection controller: its
--                       spawner `FUN_801E573C` (a0 dst, a1 src, a2/a3 +
--                       four stacked words) and every tick `FUN_801E5154`
--                       into w6c_mirror.csv
--   LEGAIA_SHOT_SCENE   comma list; take a framebuffer grab every
--                       LEGAIA_SHOT_EVERY field vsyncs (default 30) in each
--                       listed scene, at most LEGAIA_SHOT_MAX (default 12)
--                       per scene
--   LEGAIA_CKPT_SCENE / LEGAIA_CKPT_LABEL   as the poke-walk
--
-- Output: w6c_spoke_route.csv (per vsync), w6c_spoke_flags.csv (every
-- helper hit), w6c_spoke_flags_first.csv, w6c_spoke_hits.csv (tint
-- pushes), w6c_spoke.log.

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
local FLAG_BANK  = 0x80085758
local FLAG_SET_PC = 0x8003CE08
local FLAG_CLR_PC = 0x8003CE34
local POKE_FOR   = probe.getenv_num("LEGAIA_POKE_FOR", 20)
-- 1 = run LEGAIA_MASH from the first armed vsync instead of from the last
-- leg: a walk-on record's beat parks on its first text box until Cross, so
-- a route of beats needs the confirm running throughout.
local MASH_ALWAYS = probe.getenv("LEGAIA_MASH_ALWAYS", "") == "1"
local WANT_MIRROR = probe.getenv("LEGAIA_MIRROR", "") == "1"
local MIRROR_SPAWN = 0x801E573C   -- reflection controller spawner (field overlay)
local MIRROR_TICK  = 0x801E5154   -- reflection controller tick (field overlay)
local SHOT_SCENE = probe.getenv("LEGAIA_SHOT_SCENE", "")
local SHOT_EVERY = probe.getenv_num("LEGAIA_SHOT_EVERY", 30)
local SHOT_MAX   = probe.getenv_num("LEGAIA_SHOT_MAX", 12)
local shot_scenes, shots_taken, shot_field = {}, {}, {}
for sc in string.gmatch(SHOT_SCENE, "[^,%s]+") do shot_scenes[sc] = true end

-- Framebuffer grab (`PCSX.GPU.takeScreenShot`) as `<name>.raw` + `.meta`;
-- the host converts. Screenshots are Sony-derived: they stay in the
-- gitignored capture directory.
local function shot(name)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if ok and ss then
        local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
        local h = io.open(probe.out_path(name .. ".raw"), "wb"); h:write(tostring(ss.data)); h:close()
        local m = io.open(probe.out_path(name .. ".meta"), "w")
        m:write(string.format("width=%d\nheight=%d\nbpp=%d\n", tonumber(ss.width), tonumber(ss.height), bpp)); m:close()
    else
        PCSX.log("[w6c_spoke] screenshot failed: " .. tostring(ss))
    end
end

local flag_pokes = {}
for tok in string.gmatch(probe.getenv("LEGAIA_FLAG_POKE", ""), "[^,%s]+") do
    local sc, f, v = string.match(tok, "^(%w+):(%d+)=([01])$")
    if sc == nil then error("LEGAIA_FLAG_POKE entry '" .. tok .. "' is not <scene>:<flag>=<0|1>") end
    flag_pokes[#flag_pokes + 1] = { scene = sc, flag = tonumber(f), val = tonumber(v), done = false }
end

local SSTATE     = probe.getenv("LEGAIA_SSTATE", "")
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local SETTLE     = probe.getenv_num("LEGAIA_SETTLE", 90)
local POST       = probe.getenv_num("LEGAIA_FRAMES", 300)
local MAX_TICKS  = probe.getenv_num("LEGAIA_MAX_TICKS", 4000)
local CKPT_SCENE = probe.getenv("LEGAIA_CKPT_SCENE", "")
local CKPT_LABEL = probe.getenv("LEGAIA_CKPT_LABEL", "poke_walk")
local WANT_TINT  = probe.getenv("LEGAIA_TINT", "") == "1"
local OUT_DIR    = probe.getenv("LEGAIA_OUT_DIR", "captures/w6c_spoke_walk")

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
    local scene, tx, tz, stay, until_hex = string.match(tok, "^(%w+)@(%d+),(%d+)(!?)(%x*)$")
    if scene == nil then error("LEGAIA_ROUTE leg '" .. tok .. "' is not <scene>@<x>,<z>[![flaghex]]") end
    route[#route + 1] = { scene = scene, tx = tonumber(tx), tz = tonumber(tz), stay = (stay == "!"),
        until_flag = (until_hex ~= "" and tonumber(until_hex, 16) or nil) }
end

local presses = {}
for tok in string.gmatch(probe.getenv("LEGAIA_PRESS", ""), "[^,%s]+") do
    local at, name, dur = string.match(tok, "^(%d+):(%a+):(%d+)$")
    local b = name and BTN[string.lower(name)]
    if b == nil then error("LEGAIA_PRESS step '" .. tok .. "' is not <frame>:<button>:<for>") end
    presses[#presses + 1] = { at = tonumber(at), dur = tonumber(dur), btn = b, name = name }
end

local CSV = probe.csv_open(probe.out_path("w6c_spoke_route.csv"),
    "vsync,scene,mode,px,pz,tile_x,tile_z,leg,tint_r,tint_g,tint_b")
local HITS = probe.csv_open(probe.out_path("w6c_spoke_hits.csv"),
    "vsync,site,a0,a1,a2,ra,scene,mode")
local LOGF = io.open(probe.out_path("w6c_spoke.log"), "w")
local FLAGS = probe.csv_open(probe.out_path("w6c_spoke_flags.csv"),
    "vsync,op,flag_hex,flag,ra,pc_offset,op_bytes,scene,mode")
local FIRST = probe.csv_open(probe.out_path("w6c_spoke_flags_first.csv"),
    "vsync,op,flag_hex,flag,ra,pc_offset,op_bytes,scene,mode,bit_before")
local first_seen, hit_count = {}, {}
local MIRROR = WANT_MIRROR and probe.csv_open(probe.out_path("w6c_mirror.csv"),
    "vsync,event,ctrl,dst,src,w0,w1,w2,w3,w4,w5,src_x,src_y,src_z,src_h,src_64,dst_x,dst_y,dst_z,dst_h,dst_64,scene") or nil

local function log(s)
    PCSX.log("[w6c_spoke] " .. s)
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
    for k, n in pairs(hit_count) do log(string.format("count %s = %d", k, n)) end
    CSV:close(); HITS:close(); FLAGS:close(); FIRST:close()
    if MIRROR then MIRROR:close() end
    if LOGF then LOGF:close() end
    PCSX.quit(0)
end

local function flag_bit(f)
    local b = mem.read_u8(FLAG_BANK + bit.rshift(f, 3)) or 0
    return bit.band(bit.rshift(b, 7 - bit.band(f, 7)), 1)
end

local function cursor4(s8)
    if s8 < 0x80000000 or s8 >= 0x80200000 then return "-" end
    local o = {}
    for i = 0, 3 do o[#o + 1] = string.format("%02X", u8(s8 + i)) end
    return table.concat(o)
end

-- At a helper call from the field VM, `s8` is the dispatcher's pc OFFSET
-- into the executing record (`FUN_801DE840`: `move s8,a1`) and `s0` the op
-- POINTER (`addu s0,a0,s8`) - so the offset is logged as-is and the four
-- op bytes are read at `s0`.
local function flag_hit(op)
    local r = PCSX.getRegisters()
    local f = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFF)
    local ra = u32n(r.GPR.n.ra)
    local s8 = u32n(r.GPR.n.s8)
    local s0 = u32n(r.GPR.n.s0)
    local sc, md = scene_name(), u8(GAME_MODE)
    FLAGS:row("%d,%s,0x%03X,%d,%s,%s,%s,%s,0x%02X", vsync, op, f, f, hex32(ra), hex32(s8),
        cursor4(s0), sc, md)
    local key = string.format("%s 0x%03X ra=%s", op, f, hex32(ra))
    hit_count[key] = (hit_count[key] or 0) + 1
    if not first_seen[key] then
        first_seen[key] = true
        FIRST:row("%d,%s,0x%03X,%d,%s,%s,%s,%s,0x%02X,%d", vsync, op, f, f, hex32(ra),
            hex32(s8), cursor4(s0), sc, md, flag_bit(f))
        FIRST.fh:flush()
        log(string.format("first %s at f=%d scene=%s pc=+%s op=[%s]", key, vsync, sc, hex32(s8), cursor4(s0)))
    end
end

local function s16(a)
    local v = (mem.read_u16(a) or 0) % 0x10000
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

-- Reflection controller (`4C 86`): the spawner's arguments, then one row per
-- tick with the source (`+0x94`) and destination (`+0x90`) actors' position
-- (`+0x14/+0x16/+0x18`), facing (`+0x26`) and the `+0x64` word the tick
-- compares before it copies the animation. A tick row is sampled at the
-- tick's ENTRY, so its `dst` columns are what the PREVIOUS tick wrote.
local function mirror_row(event, ctrl, dst, src, w)
    MIRROR:row("%d,%s,%s,%s,%s,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%s",
        vsync, event, hex32(ctrl), hex32(dst), hex32(src), w[1], w[2], w[3], w[4], w[5], w[6],
        s16(src + 0x14), s16(src + 0x16), s16(src + 0x18), s16(src + 0x26), s16(src + 0x64),
        s16(dst + 0x14), s16(dst + 0x16), s16(dst + 0x18), s16(dst + 0x26), s16(dst + 0x64),
        scene_name())
    MIRROR.fh:flush()
end

local function arm_mirror()
    bp.arm(MIRROR_SPAWN, "Exec", 4, "mirror_spawn", function()
        local r = PCSX.getRegisters()
        local sp = u32n(r.GPR.n.sp)
        local function sx(v) v = u32n(v) % 0x10000; if v >= 0x8000 then v = v - 0x10000 end; return v end
        local w = { sx(r.GPR.n.a2), sx(r.GPR.n.a3), s16(sp + 0x10), s16(sp + 0x14), s16(sp + 0x18), s16(sp + 0x1C) }
        local dst, src = u32n(r.GPR.n.a0), u32n(r.GPR.n.a1)
        mirror_row("spawn", 0, dst, src, w)
        log(string.format("mirror spawn at f=%d dst=%s src=%s w=(%d,%d,%d,%d,%d,%d) ra=%s",
            vsync, hex32(dst), hex32(src), w[1], w[2], w[3], w[4], w[5], w[6], hex32(r.GPR.n.ra)))
    end)
    bp.arm(MIRROR_TICK, "Exec", 4, "mirror_tick", function()
        local r = PCSX.getRegisters()
        local c = u32n(r.GPR.n.a0)
        local dst, src = u32n(mem.read_u32(c + 0x90) or 0), u32n(mem.read_u32(c + 0x94) or 0)
        if dst < 0x80000000 or src < 0x80000000 then return end
        mirror_row("tick", c, dst, src, { s16(c + 0x80), s16(c + 0x82), s16(c + 0x84),
            s16(c + 0x86), s16(c + 0x88), s16(c + 0x8A) })
    end)
end

local function arm_all()
    if WANT_MIRROR then arm_mirror() end
    bp.arm(FLAG_SET_PC, "Exec", 4, "flag_set", function() flag_hit("SET") end)
    bp.arm(FLAG_CLR_PC, "Exec", 4, "flag_clr", function() flag_hit("CLR") end)
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
    if MASH_ALWAYS and mash ~= nil then mash_from = vsync end
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

    for _, fp in ipairs(flag_pokes) do
        if not fp.done and md == 0x03 and sc == fp.scene and field_ticks >= 2 then
            fp.done = true
            local a = FLAG_BANK + bit.rshift(fp.flag, 3)
            local m = bit.rshift(0x80, bit.band(fp.flag, 7))
            local b = u8(a)
            local nb = (fp.val == 1) and bit.bor(b, m) or bit.band(b, bit.bxor(m, 0xFF))
            mem.write_u8(a, nb)
            log(string.format("FLAG POKE (synthetic) 0x%03X := %d in %s at f=%d (byte 0x%02X -> 0x%02X)",
                fp.flag, fp.val, sc, vsync, b, nb))
        end
    end

    local l = route[leg]
    if l ~= nil and l.waiting then
        if flag_bit(l.until_flag) == 1 then
            log(string.format("stay leg %d: flag 0x%03X set at tick %d", leg, l.until_flag, vsync))
            leg = leg + 1
            field_ticks = 0
        end
        l = nil
    end
    if l ~= nil and md == 0x03 and sc == l.scene and field_ticks >= SETTLE then
        local p = player_ptr()
        if p ~= nil then
            mem.write_u16(p + 0x14, world_of(l.tx) % 0x10000)
            mem.write_u16(p + 0x18, world_of(l.tz) % 0x10000)
        end
        if l.stay then
            l.poked = (l.poked or 0) + 1
            if l.poked >= POKE_FOR and l.until_flag ~= nil and not l.waiting then
                l.waiting = true
                log(string.format("stay leg %d poked; waiting for flag 0x%03X", leg, l.until_flag))
            end
            if l.poked >= POKE_FOR and (l.until_flag == nil) then
                log(string.format("stay leg %d done after %d pokes at tick %d", leg, l.poked, vsync))
                leg = leg + 1
                -- Let the record's beat play out: the next leg waits a
                -- fresh LEGAIA_SETTLE of field vsyncs before it pokes.
                field_ticks = 0
            end
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
    if l ~= nil and not l.stay and sc ~= "" and sc ~= l.scene and field_ticks >= 2 then
        log(string.format("leg %d crossed: now in %s at tick %d", leg, sc, vsync))
        leg = leg + 1
        field_ticks = 0
    end

    if shot_scenes[sc] and md == 0x03 and (shots_taken[sc] or 0) < SHOT_MAX then
        shot_field[sc] = (shot_field[sc] or 0) + 1
        if shot_field[sc] % SHOT_EVERY == 1 then
            shots_taken[sc] = (shots_taken[sc] or 0) + 1
            shot(string.format("shot_%s_%05d", sc, vsync))
            log(string.format("shot %d in %s at tick %d tint=(%d,%d,%d)", shots_taken[sc], sc, vsync,
                u8(TINT_R), u8(TINT_R + 1), u8(TINT_R + 2)))
        end
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
log("=== autorun_w6c_spoke_walk ===")
log(string.format("sstate=%s route=%d legs ckpt=%s",
    SSTATE == "" and "(none)" or SSTATE, #route,
    CKPT_SCENE == "" and "(none)" or CKPT_SCENE))

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
