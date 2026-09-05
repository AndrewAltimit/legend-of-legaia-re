-- autorun_hit_event_timeline.lua
--
-- Per-vsync timeline of the retail attack band's HIT EVENTS: for every live
-- battle actor, the anim-id triple (+0x1D9/+0x1DA/+0x1DB), the event-flag
-- byte (+0x1DC), the per-clip hit index (+0x1F4), live HP (+0x14C), the
-- render node's 12.4 anim cursor (node[+0x68], node = *(actor+0x22C)) and the
-- committed clip entry's head bytes the damage kernel gates on -
-- entry[0..4] (the power run) and entry[0x10..0x14] (the hit-event frames) -
-- plus the entry's +0x76 lock byte, +0x84..+0x87 loop window and +0x0C root
-- speed. The battle ctx's action state (ctx[7]) and active actor (ctx[+0x13])
-- ride every row.
--
-- The point: `FUN_801EC3E4` is called from the anim tick every frame with
-- the cursor frame in a2 and fires one hit when `frame + 1 >=
-- entry[0x10 + hit_index]` (bumping +0x1F4). A row-by-row diff of HP against
-- the cursor and the entry's event frames is the retail oracle the engine's
-- hit-event driver is matched against.
--
-- Poll-only (no breakpoints), so it runs under --fast:
--   bash scripts/pcsx-redux/run_probe.sh --fast \
--     --lua scripts/pcsx-redux/autorun_hit_event_timeline.lua \
--     --scenario battle_vahn_tri_somersault_super --frames 700
-- Env:
--   LEGAIA_SSTATE   save state (a battle save; the scenario flag sets it)
--   LEGAIA_FRAMES   vsyncs to capture after load (default 700)
--   LEGAIA_OUT[_DIR] output CSV (default hit_event_timeline.csv)
--   LEGAIA_PRESS_AT  vsync at which to tap Cross for 6 vsyncs (0 = never;
--                    the `party_basic_attack_vs_gobu_gobu` save is parked on
--                    the Begin | Reselect confirm and needs one press)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 700)
local PRESS_AT = probe.getenv_num("LEGAIA_PRESS_AT", 0)
local OUT_PATH = probe.out_path("hit_event_timeline.csv")

local ACTOR_TABLE = 0x801C9370 -- 8 x u32 battle-actor pointers; 0..2 = party
local CTX_PTR     = 0x8007BD24 -- *(CTX_PTR) = battle ctx
local HEAP_LO     = 0x800E0000

local function u32(a) return (probe.read_u32(a) or 0) % 0x100000000 end
local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function s16(v) if v >= 0x8000 then return v - 0x10000 end return v end

local out = io.open(OUT_PATH, "w")
if not out then
    PCSX.log("[hit_event_timeline] FATAL: cannot open " .. OUT_PATH)
else
    out:write("vsync,state,active,slot,hp,a1d9,a1da,a1db,a1dc,a1de,a1dd,hit_idx,cursor,frame,",
        "p0,p1,p2,p3,e0,e1,e2,e3,lock76,loop84,loop85,loop86,solo87,speed0c,x,z\n")
end

local last = {}
local rows = 0

local function sample(vsync)
    local ctx = u32(CTX_PTR)
    if ctx < HEAP_LO then return end
    local state = u8(ctx + 7)
    local active = u8(ctx + 0x13)
    for slot = 0, 7 do
        local ap = u32(ACTOR_TABLE + slot * 4)
        if ap >= HEAP_LO and ap < 0x80200000 then
            local node = u32(ap + 0x22C)
            local cursor, entry = 0, 0
            if node >= 0x80000000 and node < 0x80200000 then
                cursor = u16(node + 0x68)
                entry = u32(node + 0x4C)
            end
            local p = { 0, 0, 0, 0 }
            local e = { 0, 0, 0, 0 }
            local lock, l84, l85, l86, solo, speed = 0, 0, 0, 0, 0, 0
            if entry >= 0x80000000 and entry < 0x80200000 then
                for i = 0, 3 do
                    p[i + 1] = u8(entry + i)
                    e[i + 1] = u8(entry + 0x10 + i)
                end
                lock = u8(entry + 0x76)
                l84, l85, l86, solo = u8(entry + 0x84), u8(entry + 0x85), u8(entry + 0x86), u8(entry + 0x87)
                speed = s16(u16(entry + 0x0C))
            end
            local line = string.format(
                "%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
                vsync, state, active, slot, s16(u16(ap + 0x14C)),
                u8(ap + 0x1D9), u8(ap + 0x1DA), u8(ap + 0x1DB), u8(ap + 0x1DC),
                u8(ap + 0x1DE), u8(ap + 0x1DD), u8(ap + 0x1F4), cursor, math.floor(cursor / 16),
                p[1], p[2], p[3], p[4], e[1], e[2], e[3], e[4],
                lock, l84, l85, l86, solo, speed,
                s16(u16(ap + 0x34)), s16(u16(ap + 0x38)))
            -- Only the vsync column changes between two quiet frames; skip
            -- exact repeats of everything else to keep the file readable.
            local key = line:gsub("^%d+,", "")
            if last[slot] ~= key then
                last[slot] = key
                if out then out:write(line, "\n") end
                rows = rows + 1
            end
        end
    end
end

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES + 8,
    on_arm = function() return {} end,
    on_capture = function(c, elapsed)
        if elapsed < 4 then return end
        if PRESS_AT > 0 then
            if elapsed == PRESS_AT then pad.force(pad.BTN.CROSS) end
            if elapsed == PRESS_AT + 6 then pad.release(pad.BTN.CROSS) end
        end
        sample(elapsed - 4)
        if elapsed % 60 == 0 and out then out:flush() end
        if elapsed - 4 >= FRAMES then
            PCSX.log(string.format("[hit_event_timeline] done: %d rows -> %s", rows, OUT_PATH))
            if out then out:close(); out = nil end
            c.request_quit = true
        end
    end,
})
