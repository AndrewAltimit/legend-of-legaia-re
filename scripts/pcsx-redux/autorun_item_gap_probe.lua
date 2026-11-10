-- autorun_item_gap_probe.lua
--
-- Does a picked-up field item still block the player? `jouinc`'s entry
-- script (P1[0]) opens the wall gap at tile (82, 39) (`4C 70 52 27 52 27`)
-- while system flag `0x5D` is clear, and the item P1[17] stands in it at
-- (10624, 5056), drawn at scale 0. Its record takes the item (`50 5D`,
-- `39 EA`) and never runs `31 00`, and the collision candidate builder
-- `FUN_801CF754` drops an actor only on `+0x10 & 3`, so the reading is that
-- the item blocks the gap before and after it is taken. This probe walks
-- into the gap on retail before the pickup, presses CROSS against the item
-- and mashes through its message, then walks into the gap twice more.
--
-- Env: LEGAIA_SSTATE (a jouinc state with 0x5D clear, e.g. the library's
--      jouinc_field_door_hop_settled), LEGAIA_OUT_DIR, LEGAIA_BOX (below).
-- Output: item_gap.csv (one row a vsync: phase, player, item +0x10, 0x5D,
--         the cull inputs: region box, tile window, stored focus)
--         and item_gap.log (the furthest east each walk reached).
-- Poll-only: safe under --fast.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local pad   = require("probe.pad")

local SSTATE     = probe.getenv("LEGAIA_SSTATE", "")
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local OUT_DIR    = probe.getenv("LEGAIA_OUT_DIR", "captures/item_gap_probe")
-- The region box (`0x1F800384..87`) a player walking into the corridor
-- carries: walking never re-latches it, so it is the box of the last landing,
-- and the corridor's way in lands at (59, 4), inside the `.MAP` region record
-- 5 (x 47..93, z 0..65, type 0). A pose leaves the state's own latch (record
-- 1, the door's room) in place, which culls the item; empty = keep it.
local BOX = {}
for v in string.gmatch(probe.getenv("LEGAIA_BOX", "47,0,93,65"), "%d+") do BOX[#BOX + 1] = tonumber(v) end
local ffi = require("ffi")

local PLAYER_PTR = 0x8007C364
local ACTOR_LIST = 0x8007C354
local SYS_FLAGS  = 0x80085758
local GAME_MODE  = 0x8007B83C
local ITEM_X, ITEM_Z = 10624, 5056
-- West of the gap, in the corridor at tile (81, 39).
local START_X, START_Z = 10432, 5056

-- The schedule, in vsyncs from the load.
local PHASES = {
    { from = 60,   to = 64,   name = "seat1" },
    { from = 64,   to = 184,  name = "walk1",  hold = "RIGHT" },
    { from = 188,  to = 192,  name = "take",   hold = "CROSS" },
    { from = 192,  to = 700,  name = "mash",   mash = "CROSS" },
    { from = 700,  to = 704,  name = "seat2" },
    { from = 704,  to = 824,  name = "walk2",  hold = "RIGHT" },
    { from = 824,  to = 828,  name = "seat3" },
    { from = 828,  to = 948,  name = "walk3",  hold = "RIGHT" },
}
local END_AT = 960

local function u32n(v) v = tonumber(v) or 0; if v < 0 then v = v + 4294967296 end; return v end
local function s16(a)
    local v = (mem.read_u16(a) or 0) % 0x10000
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end
local FOCUS_X, FOCUS_Z = 0x80089118, 0x80089120
local function s32(a)
    local v = u32n(mem.read_u32(a) or 0)
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end
local function sysflag(n)
    local b = mem.read_u8(SYS_FLAGS + math.floor(n / 8)) or 0
    return math.floor(b / 2 ^ (7 - n % 8)) % 2
end

local function find_item()
    local p = u32n(mem.read_u32(ACTOR_LIST) or 0)
    for _ = 1, 400 do
        if p < 0x80000000 or p >= 0x80200000 then return nil end
        if s16(p + 0x14) == ITEM_X and s16(p + 0x18) == ITEM_Z then return p end
        p = u32n(mem.read_u32(p) or 0)
    end
    return nil
end

local LOGF
local function log(s)
    PCSX.log("[item_gap] " .. s)
    if LOGF then LOGF:write(s .. "\n"); LOGF:flush() end
end

os.execute(string.format("mkdir -p %q", OUT_DIR))
LOGF = io.open(OUT_DIR .. "/item_gap.log", "w")
local CSV = probe.csv_open(OUT_DIR .. "/item_gap.csv", "vsync,phase,px,pz,item_flags,flag_5d,mode,box,window,focus_x,focus_z")

local vsync, loaded_at, done = 0, nil, false
local held = nil
local furthest = {}

local function release()
    if held then pad.release(pad.BTN[held]); held = nil end
end

local function finish(why)
    if done then return end
    done = true
    release()
    for _, ph in ipairs(PHASES) do
        if furthest[ph.name] then
            log(string.format("%s: furthest east x=%d", ph.name, furthest[ph.name]))
        end
    end
    log("done: " .. why)
    CSV:close()
    LOGF:close()
    PCSX.quit(0)
end

local function on_vsync()
    if done then return end
    vsync = vsync + 1
    if loaded_at == nil then
        if vsync >= BOOT_DELAY then
            if not probe.load_save_state(SSTATE) then finish("load failed"); return end
            loaded_at = vsync
            local it = find_item()
            log(string.format("loaded; item actor %s flags 0x%08X scale %d; 0x5D=%d",
                it and string.format("0x%08X", it) or "none",
                it and u32n(mem.read_u32(it + 0x10)) or 0,
                it and s16(it + 0x72) or -1, sysflag(0x5D)))
        end
        return
    end
    local t = vsync - loaded_at
    local phase = nil
    for _, ph in ipairs(PHASES) do
        if t >= ph.from and t < ph.to then phase = ph end
    end
    local p = u32n(mem.read_u32(PLAYER_PTR) or 0)
    if phase and p >= 0x80000000 and p < 0x80200000 then
        if string.sub(phase.name, 1, 4) == "seat" then
            release()
            if #BOX == 4 then
                local sp = ffi.cast("uint8_t*", PCSX.getScratchPtr())
                for i = 1, 4 do sp[0x384 + i - 1] = BOX[i] end
            end
            mem.write_u16(p + 0x14, START_X % 0x10000)
            mem.write_u16(p + 0x18, START_Z % 0x10000)
        elseif phase.hold then
            if held ~= phase.hold then release(); held = phase.hold; pad.force(pad.BTN[held]) end
        elseif phase.mash then
            release()
            if (t - phase.from) % 20 < 3 then pad.force(pad.BTN[phase.mash]); held = phase.mash end
        end
    elseif not phase then
        release()
    end
    local px, pz = s16(p + 0x14), s16(p + 0x18)
    if phase and string.sub(phase.name, 1, 4) == "walk" then
        if furthest[phase.name] == nil or px > furthest[phase.name] then furthest[phase.name] = px end
    end
    local it = find_item()
    local function sb(a)
        local v = mem.read_scratch_u8(a)
        if v >= 0x80 then v = v - 0x100 end
        return v
    end
    CSV:row("%d,%s,%d,%d,0x%08X,%d,0x%02X,%d/%d/%d/%d,%d/%d/%d/%d,%d,%d", t,
        phase and phase.name or "-", px, pz,
        it and u32n(mem.read_u32(it + 0x10)) or 0, sysflag(0x5D), mem.read_u8(GAME_MODE) or 0,
        mem.read_scratch_u8(0x1F800384), mem.read_scratch_u8(0x1F800385),
        mem.read_scratch_u8(0x1F800386), mem.read_scratch_u8(0x1F800387),
        sb(0x1F8003E8), sb(0x1F8003E9), sb(0x1F8003EA), sb(0x1F8003EB),
        s32(FOCUS_X), s32(FOCUS_Z))
    if t >= END_AT then finish("schedule complete") end
end

log("=== autorun_item_gap_probe === sstate=" .. SSTATE)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
