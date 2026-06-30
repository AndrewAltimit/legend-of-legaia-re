-- autorun_s4_gridrecon.lua
--
-- S4 grid-BFS groundwork: a pure RECON probe that pins the three things the
-- door-nav controller needs to share one coordinate frame with the engine:
--
--   1. The player position field WIDTH. docs/subsystems/field-locomotion.md
--      says player+0x14 = world X (s16), +0x16 = facing (s16), +0x18 = world Z
--      (s16). The earlier s4 probes read these as u32, which folds the facing
--      word at +0x16 into the high 16 bits of the X read - so every turn
--      corrupted the measured displacement and made a static camera look
--      "dynamic". This probe reads them as 16-bit signed and logs facing too.
--   2. The per-scene walkability grid at *(_DAT_1f8003ec)+0x4000 (1 byte / 128-
--      unit tile, 0x80-byte rows; high nibble = 4 sub-cell wall bits). It dumps
--      grid stats, the player's current tile, whether that tile reads walkable,
--      and an ASCII map of the area around the player.
--   3. The REAL pad->world mapping: it holds each of UP/RIGHT/DOWN/LEFT for a
--      window and logs the clean 16-bit (dX,dZ) plus the facing change - so we
--      can see directly whether the camera-remap is static or rotates.
--
-- No movement goal, no checkpoint - just measurement. Env: LEGAIA_SSTATE,
-- LEGAIA_OUT_DIR, LEGAIA_HOLD, LEGAIA_GAP, LEGAIA_MAP_RADIUS.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP   = 0x8001698C
local PLAYER     = 0x8007C364
local SCENE_NAME = 0x8007050C
local GM         = 0x8007B83C
local FIELDBUF_P = 0x1F8003EC      -- scratchpad pointer -> field buffer base
local GRID_OFF   = 0x4000          -- collision grid within the field buffer

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s4_gridrecon")
local HOLD       = tonumber(env.getenv("LEGAIA_HOLD", "45")) or 45
local GAP        = tonumber(env.getenv("LEGAIA_GAP", "35")) or 35
local SETTLE0    = tonumber(env.getenv("LEGAIA_SETTLE0", "60")) or 60
local MAP_R      = tonumber(env.getenv("LEGAIA_MAP_RADIUS", "22")) or 22
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/recon.log", "w")
local function log(s) PCSX.log("[recon] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru16(a) return mem.in_ram(a) and mem.read_u16(a) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end
local function s16(v)  if v == nil then return nil end; if v >= 0x8000 then return v - 0x10000 end; return v end
local function read_scene()
    local s = {}
    for i=0,7 do local b=ru8(SCENE_NAME+i) or 0; if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end

-- player position read the RIGHT way: 16-bit signed fields.
local function player_ptr() return ru32(PLAYER) end
local function ppos()
    local pp=player_ptr(); if pp==nil then return nil end
    return s16(ru16(pp+0x14)), s16(ru16(pp+0x18))     -- X, Z
end
local function pfacing()
    local pp=player_ptr(); if pp==nil then return nil end
    return s16(ru16(pp+0x16)), ru8(pp+0x26)            -- render facing, 8-dir heading
end

-- field buffer base + grid byte for a 128-unit tile (col,row).
local function gridbase()
    local b = mem.read_scratch_u32(FIELDBUF_P)
    if b == nil or b == 0 then return nil end
    return b
end
local function grid_byte(base, col, row)
    if col < 0 or col >= 0x80 or row < 0 or row >= 0x80 then return nil end
    return ru8(base + GRID_OFF + row*0x80 + col)
end
-- floor-convention tile + quadrant for a world position (no lookahead bias).
local function world_to_tile(x, z)
    return math.floor(x/128), math.floor(z/128)
end
local function tile_is_wall_floor(base, x, z)
    -- plain floor sampler convention (no +2/ceil bias): standing-tile quadrant.
    local xc = math.floor(x/64); local zc = math.floor(z/64)
    local col = math.floor(xc/2); local row = math.floor(zc/2)
    local b = grid_byte(base, col, row); if b == nil then return nil end
    local quad = 2^(((zc%2)*2) + (xc%2))           -- 1/2/4/8
    local hi = math.floor(b/16)
    return (math.floor(hi/quad) % 2) == 1, col, row, b
end

local function dump_grid(base)
    if base == nil then log("GRID: field buffer base is nil/0"); return end
    log(string.format("field buffer base = 0x%08X, grid @ 0x%08X", base, base+GRID_OFF))
    local walls, partial, empty = 0, 0, 0
    local minc,maxc,minr,maxr = 0x80,-1,0x80,-1
    for row=0,0x7f do for col=0,0x7f do
        local b = grid_byte(base,col,row) or 0
        local hi = math.floor(b/16)
        if hi == 0 then empty = empty+1
        elseif hi == 0xF then walls = walls+1
        else partial = partial+1 end
        if hi ~= 0xF then
            if col<minc then minc=col end; if col>maxc then maxc=col end
            if row<minr then minr=row end; if row>maxr then maxr=row end
        end
    end end
    log(string.format("grid nibble census: empty=%d partial=%d full-wall=%d", empty, partial, walls))
    log(string.format("non-full-wall bbox: col[%d..%d] row[%d..%d]", minc,maxc,minr,maxr))
end

local function dump_map(base, pcol, prow)
    if base == nil then return end
    log(string.format("ASCII map (#=full wall, +=partial, .=open, P=player) around tile (%d,%d):", pcol, prow))
    log("     " .. (function() local h={}; for c=pcol-MAP_R,pcol+MAP_R do h[#h+1]=string.format("%d", math.abs(c)%10) end; return table.concat(h) end)())
    for row=prow-MAP_R,prow+MAP_R do
        local line = {}
        for col=pcol-MAP_R,pcol+MAP_R do
            local ch
            if col==pcol and row==prow then ch="P"
            else
                local b = grid_byte(base,col,row)
                if b==nil then ch=" " else
                    local hi=math.floor(b/16)
                    if hi==0 then ch="." elseif hi==0xF then ch="#" else ch="+" end
                end
            end
            line[#line+1]=ch
        end
        log(string.format("%4d %s", row, table.concat(line)))
    end
end

local vsync, loaded = 0, false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync=vsync+1
    if not loaded and START_SAVE~="" and vsync>=START_DELAY then
        loaded=true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
end)

local DIRS = { "UP","RIGHT","DOWN","LEFT" }
local frame, dumped = 0, false
local stage, stage_start = 0, nil
local held, x0, z0, f0 = nil, nil, nil, nil

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame=frame+1
    if frame<=SETTLE0 then return end

    if not dumped then
        dumped=true
        local sc=read_scene(); local gm=ru8(GM) or 0xFF
        local x,z=ppos(); local rf,hd=pfacing()
        log(string.format("scene=%q mode=0x%02X player=0x%08X", sc, gm, player_ptr() or 0))
        log(string.format("pos16  X=%s Z=%s  facing=%s heading=%s", tostring(x),tostring(z),tostring(rf),tostring(hd)))
        -- for comparison, the BUGGY u32 read the old probes used:
        local pp=player_ptr()
        if pp then log(string.format("pos32(buggy) X=%d Z=%d (folds facing word)", (ru32(pp+0x14) or 0), (ru32(pp+0x18) or 0))) end
        local base=gridbase()
        dump_grid(base)
        if x and z and base then
            local pcol,prow=world_to_tile(x,z)
            local wall,col,row,b=tile_is_wall_floor(base,x,z)
            log(string.format("player tile (col,row)=(%d,%d) gridbyte=0x%02X floor-walkable=%s",
                col or pcol, row or prow, b or 0, tostring(wall==false)))
            dump_map(base, pcol, prow)
        end
        stage_start=frame; stage=1; log("=== begin pad->world measurement (clean 16-bit) ===")
        return
    end

    -- clean per-direction displacement measurement
    local cyc=HOLD+GAP
    local el=frame-stage_start
    local idx=math.floor(el/cyc)+1
    if idx>#DIRS then
        if held then pad.release(pad.BTN[held]) end
        log("=== gridrecon done ===")
        if LOG then LOG:close() end; PCSX.quit(0); return
    end
    local ph=el%cyc; local dir=DIRS[idx]
    if ph==0 then
        x0,z0=ppos(); local rf=pfacing(); f0=rf
        if held then pad.release(pad.BTN[held]) end
        pad.force(pad.BTN[dir]); held=dir
    elseif ph==HOLD then
        pad.release(pad.BTN[dir]); held=nil
        local x1,z1=ppos(); local rf1=pfacing()
        if x0 and x1 then
            log(string.format("%-6s dX=%-5d dZ=%-5d  facing %s->%s  (%d,%d)->(%d,%d)",
                dir, x1-x0, z1-z0, tostring(f0), tostring(rf1), x0,z0,x1,z1))
        end
    end
end)

log("s4 gridrecon armed")
