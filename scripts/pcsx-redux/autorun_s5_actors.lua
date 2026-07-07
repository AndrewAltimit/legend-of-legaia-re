-- autorun_s5_actors.lua
--
-- S5 recon: town01 has no random encounters (an empty wander confirmed it), so
-- the first battle is the SCRIPTED Tetsu sparring tutorial (formation_id 4),
-- started by talking to the Rim Elm sparring partner. This probe dumps the live
-- active-actor table at the s4 exterior anchor to find that partner: the table
-- base is DAT_801c93c8 (field overlay), count _DAT_8007b6b8 (<= 0x20). For each
-- actor it logs position (player+0x14/+0x18 read as s16), the flags word +0x10
-- (Tetsu/moving-class NPCs carry bit 0x20000; the captured Tetsu = 0x08020884),
-- the heading +0x26 and object index +0x60. Also dumps the player tile and the
-- actor Vahn last interacted with (player+0x98). Pure observation - no input.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_SETTLE0.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP   = 0x8001698C
local PLAYER     = 0x8007C364
local SCENE_NAME = 0x8007050C
local GM         = 0x8007B83C
local ACTOR_TBL  = 0x801C93C8
local ACTOR_CNT  = 0x8007B6B8
local TILE       = 128

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s5_actors")
local SETTLE0    = tonumber(env.getenv("LEGAIA_SETTLE0", "60")) or 60
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/actors.log", "w")
local function log(s) PCSX.log("[act] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru16(a) return mem.in_ram(a) and mem.read_u16(a) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end
local function s16(v)  if v == nil then return nil end; if v >= 0x8000 then return v - 0x10000 end; return v end
local function read_scene()
    local s = {}
    for i=0,7 do local b=ru8(SCENE_NAME+i) or 0; if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end
local function actor_pos(p) return s16(ru16(p+0x14)), s16(ru16(p+0x18)) end

local vsync, loaded = 0, false
-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync=vsync+1
    if not loaded and START_SAVE~="" and vsync>=START_DELAY then
        loaded=true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
end)

local frame, dumped = 0, false
bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame=frame+1
    if frame<=SETTLE0 or dumped then return end
    dumped=true

    local pp = ru32(PLAYER) or 0
    local px,pz = actor_pos(pp)
    local it = ru32(pp+0x98) or 0
    log(string.format("scene=%q mode=0x%02X player=0x%08X pos=(%s,%s) tile=(%s,%s) last_interact=0x%08X",
        read_scene(), ru8(GM) or 0, pp, tostring(px),tostring(pz),
        tostring(px and math.floor(px/TILE)), tostring(pz and math.floor(pz/TILE)), it))

    local cnt = ru8(ACTOR_CNT) or 0
    log(string.format("active-actor count _DAT_8007b6b8 = %d (table 0x%08X)", cnt, ACTOR_TBL))
    for i=0,(cnt>0 and cnt-1 or 0x1f) do
        local ap = ru32(ACTOR_TBL + i*4) or 0
        if ap ~= 0 and mem.in_ram(ap+0x60) then
            local ax,az = actor_pos(ap)
            local fl = ru32(ap+0x10) or 0
            local hd = ru8(ap+0x26) or 0
            local obj = ru8(ap+0x60) or 0
            local st = ru8(ap+0x50) or 0
            local moving = (math.floor(fl/0x20000)%2==1) and "MOVING" or ""
            local isplayer = (ap==pp) and "PLAYER" or ""
            local istetsu = (fl==0x08020884) and "<<TETSU?" or ""
            log(string.format("  [%2d] @0x%08X pos=(%5s,%5s) tile=(%3s,%3s) flags=0x%08X hd=0x%02X obj=%3d st=0x%02X %s%s%s",
                i, ap, tostring(ax),tostring(az),
                tostring(ax and math.floor(ax/TILE)), tostring(az and math.floor(az/TILE)),
                fl, hd, obj, st, isplayer, moving, istetsu))
        end
    end
    log("=== actor dump done ===")
    if LOG then LOG:close() end; PCSX.quit(0)
end)

log("s5 actors armed")
