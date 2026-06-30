-- autorun_s4_doornav.lua
--
-- S4 capture: a GRID-BFS door-navigation controller. The s3_rimelm_freeroam
-- anchor spawns the player inside a walled room (Vahn's house interior in
-- town01). This probe walks the player to a scene transition deterministically:
--
--   1. Read the per-scene walkability grid at *(_DAT_1f8003ec)+0x4000 (1 byte /
--      128-unit tile; high nibble = 4 sub-cell wall bits) and the player tile
--      from player+0x14/+0x18 read as 16-BIT SIGNED (NOT u32 - the high 16 bits
--      hold the facing word +0x16; reading u32 corrupts every measurement and
--      was what made the earlier nav look impossible).
--   2. BFS the reachable walkable tiles from the player tile; collect the
--      BOUNDARY tiles (reachable tiles touching a wall) - door triggers live on
--      the edge of the walkable region. Visit them nearest-first.
--   3. Follow each BFS path with ONLINE-ADAPTIVE pad input: per pad button keep
--      an EMA of its observed world (dX,dZ) (clean 16-bit), and each frame press
--      the button whose direction best matches the vector to the next path tile
--      (handles a rotated camera). Pulse CROSS throughout (walk-touch doors +
--      NPC story triggers). At each boundary tile, also nudge toward the
--      adjacent wall - that is where a walk-touch warp fires.
--   4. A transition = the scene name leaves "town01" OR the player position
--      jumps > a tile-and-a-half in a single field tick (an intra-town warp).
--      On transition, settle and checkpoint a raw save state.
--
-- This is a faithful playthrough: only D-pad + the interact button, real game
-- physics + collision. No position pokes.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_CKPT_LABEL, LEGAIA_HOME_SCENE,
--      LEGAIA_SETTLE, LEGAIA_MAX_FRAMES, LEGAIA_STUCK, LEGAIA_NUDGE.

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
local FIELDBUF_P = 0x1F8003EC
local GRID_OFF   = 0x4000
local TILE       = 128

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s4_doornav")
local CKPT_LABEL = env.getenv("LEGAIA_CKPT_LABEL", "s4_transition")
local HOME_SCENE = env.getenv("LEGAIA_HOME_SCENE", "town01")
local SETTLE     = tonumber(env.getenv("LEGAIA_SETTLE", "25")) or 25
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "9000")) or 9000
local STUCK_LIM  = tonumber(env.getenv("LEGAIA_STUCK", "45")) or 45
local NUDGE_LEN  = tonumber(env.getenv("LEGAIA_NUDGE", "16")) or 16
local SETTLE0    = tonumber(env.getenv("LEGAIA_SETTLE0", "45")) or 45
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2
local WARP_JUMP  = tonumber(env.getenv("LEGAIA_WARP_JUMP", "300")) or 300

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/doornav.log", "w")
local function log(s) PCSX.log("[nav] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru16(a) return mem.in_ram(a) and mem.read_u16(a) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end
local function s16(v)  if v == nil then return nil end; if v >= 0x8000 then return v - 0x10000 end; return v end
local function read_scene()
    local s = {}
    for i=0,7 do local b=ru8(SCENE_NAME+i) or 0; if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end
local function player_ptr() return ru32(PLAYER) end
local function ppos()
    local pp=player_ptr(); if pp==nil then return nil end
    return s16(ru16(pp+0x14)), s16(ru16(pp+0x18))
end
local function engaged()
    local pp=player_ptr(); if pp==nil then return false end
    local fl=ru32(pp+0x10); if fl==nil then return false end
    return math.floor(fl/0x80000)%2==1
end
local function gridbase()
    local b = mem.read_scratch_u32(FIELDBUF_P)
    if b == nil or b == 0 then return nil end
    return b
end
local function grid_byte(base, col, row)
    if col < 0 or col >= 0x80 or row < 0 or row >= 0x80 then return nil end
    return ru8(base + GRID_OFF + row*0x80 + col)
end
-- tile is "walkable" for pathing if it is not a full wall (any open sub-cell).
local function tile_walkable(base, col, row)
    local b = grid_byte(base, col, row); if b == nil then return false end
    return math.floor(b/16) ~= 0xF
end
local function tile_of(x, z) return math.floor(x/TILE), math.floor(z/TILE) end
local function tile_center(col, row) return col*TILE + TILE/2, row*TILE + TILE/2 end

local function write_checkpoint(label)
    local ok=pcall(function()
        local w=PCSX.createSaveState()
        local fh=Support.File.open(OUT_DIR.."/"..label..".rawsstate","CREATE"); fh:writeMoveSlice(w); fh:close()
        log("checkpoint written: "..OUT_DIR.."/"..label..".rawsstate")
    end)
    if not ok then log("checkpoint FAILED") end
end

-- ---------------- pad <-> world model (online-adaptive) ----------------
-- bootstrap from the recon: RIGHT->+X, LEFT->-X, UP->+Z, DOWN->-Z.
local est = {
    UP    = { dx=0,    dz=1,   n=1 },
    DOWN  = { dx=0,    dz=-1,  n=1 },
    LEFT  = { dx=-1,   dz=0,   n=1 },
    RIGHT = { dx=1,    dz=0,   n=1 },
}
local DIRS = { "UP","DOWN","LEFT","RIGHT" }
local function unit(dx,dz) local m=math.sqrt(dx*dx+dz*dz); if m<1e-6 then return 0,0 end; return dx/m,dz/m end
local function best_button(wx, wz)
    local ux,uz = unit(wx,wz)
    local bestd, bestb = -2, "RIGHT"
    for _,b in ipairs(DIRS) do
        local ex,ez = unit(est[b].dx, est[b].dz)
        local d = ex*ux + ez*uz
        if d > bestd then bestd=d; bestb=b end
    end
    return bestb, bestd
end
local function update_est(btn, dx, dz)
    if btn==nil then return end
    if math.abs(dx)+math.abs(dz) < 6 then return end   -- ignore wall-blocked / noise
    local e = est[btn]
    local a = 0.3
    e.dx = (1-a)*e.dx + a*dx
    e.dz = (1-a)*e.dz + a*dz
end

-- ---------------- BFS over the walkable grid ----------------
local function key(c,r) return r*0x80 + c end
local function bfs(base, sc, sr)
    local prev = {}            -- key -> {c,r} parent
    local dist = {}
    local order = {}
    local q = { {sc,sr} }
    dist[key(sc,sr)] = 0
    local head = 1
    while head <= #q do
        local cur = q[head]; head = head + 1
        local c,r = cur[1], cur[2]
        order[#order+1] = {c,r}
        local nb = { {c+1,r},{c-1,r},{c,r+1},{c,r-1} }
        for _,n in ipairs(nb) do
            local nc,nr = n[1], n[2]
            local k = key(nc,nr)
            if dist[k]==nil and tile_walkable(base, nc, nr) then
                dist[k] = dist[key(c,r)] + 1
                prev[k] = {c,r}
                q[#q+1] = {nc,nr}
            end
        end
    end
    return prev, dist, order
end
-- reconstruct tile path from (sc,sr) to (tc,tr) using prev.
local function path_to(prev, sc, sr, tc, tr)
    local p = {}
    local c,r = tc,tr
    while not (c==sc and r==sr) do
        p[#p+1] = {c,r}
        local pr = prev[key(c,r)]
        if pr==nil then return nil end
        c,r = pr[1], pr[2]
    end
    -- p is target..first-step; reverse to first-step..target
    local out = {}
    for i=#p,1,-1 do out[#out+1]=p[i] end
    return out
end
-- boundary tiles: reachable tiles with >=1 non-walkable 4-neighbour, sorted by dist.
local function boundary_targets(base, dist, order)
    local b = {}
    for _,t in ipairs(order) do
        local c,r = t[1],t[2]
        local edge = false
        for _,n in ipairs({ {c+1,r},{c-1,r},{c,r+1},{c,r-1} }) do
            if not tile_walkable(base, n[1], n[2]) then edge=true; break end
        end
        if edge then b[#b+1] = {c=c, r=r, d=dist[key(c,r)] or 1e9} end
    end
    table.sort(b, function(a,b2) return a.d < b2.d end)
    return b
end

-- ---------------- state machine ----------------
local vsync, loaded = 0, false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync=vsync+1
    if not loaded and START_SAVE~="" and vsync>=START_DELAY then
        loaded=true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
    end
end)

local held = {}
local function release_all() for _,b in ipairs(DIRS) do if held[b] then pad.release(pad.BTN[b]); held[b]=nil end end end
local function hold_only(btn)
    for _,b in ipairs(DIRS) do
        if b==btn then if not held[b] then pad.force(pad.BTN[b]); held[b]=true end
        else if held[b] then pad.release(pad.BTN[b]); held[b]=false end end
    end
end

local frame = 0
local phase = "INIT"
local base = nil
local targets, ti = nil, 1
local path, pi = nil, 1
local cur_btn = nil
local lastx, lastz = nil, nil
local stuck = 0
local cross_state, cross_t = 0, 0
local nudge_left, nudge_dir = 0, nil
local settle_since = nil
local quit_at = nil

local function plan_to(tc, tr)
    local x,z = ppos(); if x==nil then return false end
    local sc, sr = tile_of(x,z)
    local prev, dist, order = bfs(base, sc, sr)
    if dist[key(tc,tr)] == nil then return false end
    path = path_to(prev, sc, sr, tc, tr)
    pi = 1
    return path ~= nil and #path > 0
end

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame=frame+1
    if frame<=SETTLE0 then return end
    if frame>=MAX_FRAMES then release_all(); log(string.format("MAX_FRAMES, scene=%q", read_scene())); if LOG then LOG:close() end; PCSX.quit(0); return end

    -- universal transition watcher (scene change or warp jump)
    local sc = read_scene()
    local x,z = ppos()
    if phase~="SETTLE_NEW" and phase~="DONE" then
        local warped = false
        if sc~=HOME_SCENE and sc~="" then warped = true; log(string.format("[f%d] scene change %q->%q", frame, HOME_SCENE, sc)) end
        if x and lastx and (math.abs(x-lastx)+math.abs(z-lastz)) > WARP_JUMP then
            warped = true; log(string.format("[f%d] WARP jump (%d,%d)->(%d,%d) d=%d", frame, lastx,lastz,x,z, math.abs(x-lastx)+math.abs(z-lastz)))
        end
        if warped then release_all(); phase="SETTLE_NEW"; settle_since=nil; lastx,lastz=x,z; return end
    end

    if phase=="INIT" then
        base = gridbase()
        if base==nil then return end
        local px,pz = ppos(); if px==nil then return end
        local pc,pr = tile_of(px,pz)
        local prev, dist, order = bfs(base, pc, pr)
        targets = boundary_targets(base, dist, order)
        log(string.format("INIT: player tile (%d,%d), %d reachable, %d boundary targets", pc,pr, #order, #targets))
        ti = 1; phase="NEXT_TARGET"
        lastx,lastz = px,pz
        return
    end

    -- dialogue dismissal (an NPC talk opened a box): pulse CROSS until clear.
    if engaged() then
        release_all()
        if cross_state==1 and frame>=cross_t then pad.release(pad.BTN.CROSS); cross_state=0; cross_t=frame+6
        elseif cross_state==0 and frame>=cross_t then pad.force(pad.BTN.CROSS); cross_state=1; cross_t=frame+3 end
        -- a story talk may itself be the S4 trigger (mode/scene change caught above)
        return
    end

    if phase=="NEXT_TARGET" then
        if targets==nil or ti>#targets then
            release_all(); log("exhausted all boundary targets without a transition");
            -- re-BFS once more in case paints opened new tiles; else give up
            if LOG then LOG:close() end; PCSX.quit(0); return
        end
        local t = targets[ti]
        if plan_to(t.c, t.r) then
            phase="FOLLOW"; stuck=0; nudge_left=0
            if (ti%10)==1 then log(string.format("target %d/%d tile (%d,%d) d=%d, path len %d", ti,#targets,t.c,t.r,t.d,#path)) end
        else
            ti=ti+1   -- unreachable now, skip
        end
        return
    end

    if phase=="FOLLOW" then
        -- pulse CROSS while moving to fire walk-touch doors / talk to NPCs
        if cross_state==1 and frame>=cross_t then pad.release(pad.BTN.CROSS); cross_state=0; cross_t=frame+12
        elseif cross_state==0 and frame>=cross_t then pad.force(pad.BTN.CROSS); cross_state=1; cross_t=frame+2 end

        -- update online estimate from last frame's motion
        if cur_btn and lastx then update_est(cur_btn, x-lastx, z-lastz) end

        if nudge_left>0 then
            hold_only(nudge_dir); nudge_left=nudge_left-1
            lastx,lastz = x,z
            if nudge_left==0 then ti=ti+1; phase="NEXT_TARGET" end
            return
        end

        local px,pz = x,z
        local cc,cr = tile_of(px,pz)
        -- reached current waypoint?
        while pi<=#path and cc==path[pi][1] and cr==path[pi][2] do pi=pi+1 end
        if pi>#path then
            -- arrived at the boundary tile: nudge toward the adjacent wall(s)
            local t = targets[ti]
            local nd = nil
            for _,cand in ipairs({ {1,0,"RIGHT"},{-1,0,"LEFT"},{0,1,"DOWN_Z"},{0,-1,"UP_Z"} }) do
                if not tile_walkable(base, t.c+cand[1], t.r+cand[2]) then
                    -- world direction toward that wall
                    local b = best_button(cand[1]*TILE, cand[2]*TILE)
                    nd = b; break
                end
            end
            nudge_dir = nd or "UP"
            nudge_left = NUDGE_LEN
            return
        end

        -- steer toward next waypoint centre
        local tc,tr = path[pi][1], path[pi][2]
        local wx,wz = tile_center(tc,tr)
        local desx,desz = wx-px, wz-pz
        local btn = best_button(desx, desz)
        hold_only(btn); cur_btn = btn

        -- stuck detection (no net movement)
        if lastx and math.abs(px-lastx)+math.abs(pz-lastz) < 2 then stuck=stuck+1 else stuck=0 end
        if stuck>=STUCK_LIM then
            release_all(); stuck=0; ti=ti+1; phase="NEXT_TARGET"   -- give up this target
        end
        lastx,lastz = px,pz
        return
    end

    if phase=="SETTLE_NEW" then
        release_all()
        local m = ru8(GM) or 0xFF
        local stable = (sc~="" ) and not (sc==HOME_SCENE and x and lastx and (math.abs(x-lastx)+math.abs(z-lastz))>WARP_JUMP)
        if m==0x03 and stable then
            if settle_since==nil then settle_since=frame
            elseif frame-settle_since>=SETTLE then
                log(string.format("[f%d] settled: scene=%q mode=0x%02X pos=(%s,%s); checkpointing", frame, sc, m, tostring(x), tostring(z)))
                write_checkpoint(CKPT_LABEL); phase="DONE"; quit_at=frame+2
            end
        else settle_since=nil end
        lastx,lastz = x,z
        return
    end

    if phase=="DONE" and quit_at and frame>=quit_at then if LOG then LOG:close() end; PCSX.quit(0) end
end)

log("s4 doornav armed")
