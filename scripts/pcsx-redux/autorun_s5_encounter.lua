-- autorun_s5_encounter.lua
--
-- S5 groundwork + capture attempt: from the s4_rimelm_door_transition exterior
-- anchor, reach the FIRST BATTLE. Two ways a battle can start here:
--   (a) a random encounter while walking (town01's MAN declares formations), or
--   (b) the scripted Tetsu sparring tutorial, started by talking to the Rim Elm
--       sparring partner (CROSS at the NPC -> dialogue-accept auto-arm).
-- This probe does both at once: it WANDERS the walkable area (grid-BFS to the
-- farthest reachable tile, repeat; re-BFS on any door warp) to accumulate steps,
-- pulsing CROSS to interact with any NPC it passes. It watches for a battle
-- (game_mode 0x8007B83C == 0x15, OR the battle-context pointer 0x8007BD24 != 0)
-- and, on the first one, settles and checkpoints an S5 battle-entry anchor.
--
-- Heavy logging (this is also recon: does a starting town roll encounters, or is
-- the first fight the scripted tutorial?): periodic status, every engaged-flag
-- transition + interaction target, every warp, and the battle trigger.
--
-- Faithful: D-pad + interact button only, real collision. No pokes.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_CKPT_LABEL, LEGAIA_HOME_SCENE,
--      LEGAIA_SETTLE, LEGAIA_MAX_FRAMES, LEGAIA_STUCK.

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
local BATTLE_CTX = 0x8007BD24
local FIELDBUF_P = 0x1F8003EC
local GRID_OFF   = 0x4000
local TILE       = 128
local BATTLE_MODE= 0x15

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s5_encounter")
local CKPT_LABEL = env.getenv("LEGAIA_CKPT_LABEL", "s5_battle_entry")
local HOME_SCENE = env.getenv("LEGAIA_HOME_SCENE", "town01")
local SETTLE     = tonumber(env.getenv("LEGAIA_SETTLE", "20")) or 20
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "12000")) or 12000
local STUCK_LIM  = tonumber(env.getenv("LEGAIA_STUCK", "45")) or 45
local SETTLE0    = tonumber(env.getenv("LEGAIA_SETTLE0", "45")) or 45
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2
local WARP_JUMP  = tonumber(env.getenv("LEGAIA_WARP_JUMP", "300")) or 300

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/s5.log", "w")
local function log(s) PCSX.log("[s5] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
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
local function interact_target()
    local pp=player_ptr(); if pp==nil then return nil end
    return ru32(pp+0x98)
end
local function in_battle()
    local m=ru8(GM) or 0
    local bc=ru32(BATTLE_CTX) or 0
    return (m==BATTLE_MODE) or (bc~=0), m, bc
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

-- pad<->world model (online-adaptive; bootstrap RIGHT->+X / UP->+Z)
local est = {
    UP={dx=0,dz=1}, DOWN={dx=0,dz=-1}, LEFT={dx=-1,dz=0}, RIGHT={dx=1,dz=0},
}
local DIRS = { "UP","DOWN","LEFT","RIGHT" }
local function unit(dx,dz) local m=math.sqrt(dx*dx+dz*dz); if m<1e-6 then return 0,0 end; return dx/m,dz/m end
local function best_button(wx, wz)
    local ux,uz = unit(wx,wz); local bestd,bestb=-2,"RIGHT"
    for _,b in ipairs(DIRS) do local ex,ez=unit(est[b].dx,est[b].dz); local d=ex*ux+ez*uz
        if d>bestd then bestd=d; bestb=b end end
    return bestb
end
local function update_est(btn, dx, dz)
    if btn==nil or math.abs(dx)+math.abs(dz)<6 then return end
    local e=est[btn]; local a=0.3; e.dx=(1-a)*e.dx+a*dx; e.dz=(1-a)*e.dz+a*dz
end

-- BFS over walkable grid
local function key(c,r) return r*0x80 + c end
local function bfs(base, sc, sr)
    local prev,dist,order = {},{},{}
    local q={{sc,sr}}; dist[key(sc,sr)]=0; local head=1
    while head<=#q do
        local cur=q[head]; head=head+1; local c,r=cur[1],cur[2]; order[#order+1]={c,r}
        for _,n in ipairs({ {c+1,r},{c-1,r},{c,r+1},{c,r-1} }) do
            local nc,nr=n[1],n[2]; local k=key(nc,nr)
            if dist[k]==nil and tile_walkable(base,nc,nr) then
                dist[k]=dist[key(c,r)]+1; prev[k]={c,r}; q[#q+1]={nc,nr} end
        end
    end
    return prev,dist,order
end
local function path_to(prev, sc,sr, tc,tr)
    local p={}; local c,r=tc,tr
    while not (c==sc and r==sr) do p[#p+1]={c,r}; local pr=prev[key(c,r)]; if pr==nil then return nil end; c,r=pr[1],pr[2] end
    local out={}; for i=#p,1,-1 do out[#out+1]=p[i] end; return out
end

-- visit-count weighted farthest-tile picker (wander wide to rack up steps)
local visits = {}
local function pick_wander_target(prev, dist, order)
    local best, bestscore = nil, -1e18
    for _,t in ipairs(order) do
        local c,r=t[1],t[2]; local d=dist[key(c,r)] or 0
        local v=visits[key(c,r)] or 0
        local score = d - v*40            -- far, and not recently visited
        if score>bestscore then bestscore=score; best={c,r} end
    end
    return best
end

-- shared battle-capture state (driven from BOTH handlers; the Vsync handler is
-- authoritative because the field tick stops firing once mode flips to battle).
local battle_seen = false
local done = false
local cap_since = nil

local vsync, loaded = 0, false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync=vsync+1
    if not loaded and START_SAVE~="" and vsync>=START_DELAY then
        loaded=true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE))
        return
    end
    if not loaded or done then return end
    -- battle capture (works after the field tick stops): require SETTLE vsyncs of
    -- continuous battle residency, then checkpoint + quit.
    local b, m, bc = in_battle()
    if b then
        if not battle_seen then
            battle_seen=true
            log(string.format("*** [v%d] BATTLE detected mode=0x%02X battle_ctx=0x%08X steps~%d ***", vsync, m, bc, steps))
        end
        if cap_since==nil then cap_since=vsync
        elseif vsync-cap_since>=SETTLE then
            log(string.format("[v%d] battle settled (mode=0x%02X ctx=0x%08X); checkpointing", vsync, m, bc))
            write_checkpoint(CKPT_LABEL); done=true
            -- give the writer a beat, then quit
            log("done; quitting"); if LOG then LOG:close() end; PCSX.quit(0)
        end
    else
        cap_since=nil
    end
end)

local held={}
local function release_all() for _,b in ipairs(DIRS) do if held[b] then pad.release(pad.BTN[b]); held[b]=nil end end end
local function hold_only(btn)
    for _,b in ipairs(DIRS) do
        if b==btn then if not held[b] then pad.force(pad.BTN[b]); held[b]=true end
        else if held[b] then pad.release(pad.BTN[b]); held[b]=false end end
    end
end

local frame=0
local phase="INIT"
local base=nil
local path, pi = nil, 1
local cur_btn=nil
local lastx,lastz=nil,nil
local stuck=0
local cross_state,cross_t=0,0
local settle_since,quit_at=nil,nil
local steps=0
local was_engaged=false
local last_tilekey=nil
-- recalibration sub-phase (re-derive pad->world when the camera orientation
-- changed, e.g. after a warp into a new area with a different yaw)
local recal_i, recal_start = 0, nil
local recal_x0, recal_z0 = nil, nil
local RECAL_HOLD = 14
local stuck_runs = 0

local function replan(base)
    local x,z=ppos(); if x==nil then return false end
    local sc,sr=tile_of(x,z)
    local prev,dist,order=bfs(base,sc,sr)
    local tgt=pick_wander_target(prev,dist,order)
    if tgt==nil then return false end
    path=path_to(prev,sc,sr,tgt[1],tgt[2]); pi=1
    return path~=nil and #path>0
end

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    if done then return end
    frame=frame+1
    if frame<=SETTLE0 then return end
    if frame>=MAX_FRAMES then release_all(); log(string.format("MAX_FRAMES, no battle. scene=%q steps~%d", read_scene(), steps)); if LOG then LOG:close() end; PCSX.quit(0); return end

    -- battle started? stop walking; the Vsync handler captures + quits.
    if battle_seen or in_battle() then release_all(); return end

    local x,z = ppos()
    local sc = read_scene()

    if phase=="INIT" then
        base=gridbase(); if base==nil then return end
        local px,pz=ppos(); if px==nil then return end
        local pc,pr=tile_of(px,pz)
        log(string.format("INIT scene=%q mode=0x%02X player tile (%d,%d) battle_ctx=0x%08X", sc, ru8(GM) or 0, pc,pr, ru32(BATTLE_CTX) or 0))
        if not replan(base) then log("INIT: replan failed"); return end
        phase="WANDER"; lastx,lastz=px,pz; last_tilekey=key(pc,pr)
        return
    end

    -- log engaged transitions (dialogue / interaction); a talk may start a fight
    local eng = engaged()
    if eng ~= was_engaged then
        local it = interact_target() or 0
        log(string.format("[f%d] engaged %s->%s interact_target=0x%08X tile=(%s,%s)", frame, tostring(was_engaged), tostring(eng), it, tostring(x and math.floor(x/TILE)), tostring(z and math.floor(z/TILE))))
        was_engaged = eng
    end

    -- dialogue dismissal (keep tapping CROSS to advance any opened box)
    if eng then
        release_all()
        if cross_state==1 and frame>=cross_t then pad.release(pad.BTN.CROSS); cross_state=0; cross_t=frame+6
        elseif cross_state==0 and frame>=cross_t then pad.force(pad.BTN.CROSS); cross_state=1; cross_t=frame+3 end
        return
    end

    -- RECAL: press each direction briefly, measure clean (dX,dZ), rebuild est.
    if phase=="RECAL" then
        if recal_i==0 then recal_i=1; recal_start=frame; recal_x0,recal_z0=x,z; hold_only(DIRS[1]); return end
        local d=DIRS[recal_i]
        if frame-recal_start>=RECAL_HOLD then
            local dx=(x or 0)-(recal_x0 or 0); local dz=(z or 0)-(recal_z0 or 0)
            if math.abs(dx)+math.abs(dz)>=4 then est[d]={dx=dx,dz=dz} end
            recal_i=recal_i+1
            if recal_i>#DIRS then
                release_all(); recal_i=0
                log(string.format("[f%d] RECAL done: UP(%d,%d) DOWN(%d,%d) LEFT(%d,%d) RIGHT(%d,%d)", frame,
                    est.UP.dx,est.UP.dz, est.DOWN.dx,est.DOWN.dz, est.LEFT.dx,est.LEFT.dz, est.RIGHT.dx,est.RIGHT.dz))
                if not replan(base) then phase="INIT" else phase="WANDER" end
                stuck=0; lastx,lastz=x,z
            else
                recal_start=frame; recal_x0,recal_z0=x,z; hold_only(DIRS[recal_i])
            end
        end
        return
    end

    if phase=="WANDER" then
        -- count steps (tile crossings) for the recon log
        if x then local tk=key(tile_of(x,z)); if tk~=last_tilekey then steps=steps+1; visits[tk]=(visits[tk] or 0)+1; last_tilekey=tk end end

        -- warp? (door) -> recalibrate the pad->world map for the new area, re-BFS
        if x and lastx and (math.abs(x-lastx)+math.abs(z-lastz))>WARP_JUMP then
            log(string.format("[f%d] warp (%d,%d)->(%d,%d); RECAL+re-BFS", frame, lastx,lastz,x,z))
            release_all(); lastx,lastz=x,z; stuck=0; stuck_runs=0; phase="RECAL"; recal_i=0; return
        end

        -- pulse CROSS while moving (interact with NPCs / start tutorial)
        if cross_state==1 and frame>=cross_t then pad.release(pad.BTN.CROSS); cross_state=0; cross_t=frame+10
        elseif cross_state==0 and frame>=cross_t then pad.force(pad.BTN.CROSS); cross_state=1; cross_t=frame+2 end

        if cur_btn and lastx then update_est(cur_btn, x-lastx, z-lastz) end

        if path==nil or pi>#path then if not replan(base) then phase="INIT" end; return end
        local px,pz=x,z
        local cc,cr=tile_of(px,pz)
        while pi<=#path and cc==path[pi][1] and cr==path[pi][2] do pi=pi+1 end
        if pi>#path then if not replan(base) then phase="INIT" end; return end

        local tc,tr=path[pi][1],path[pi][2]
        local wx,wz=tile_center(tc,tr)
        local btn=best_button(wx-px, wz-pz)
        hold_only(btn); cur_btn=btn

        if lastx and math.abs(px-lastx)+math.abs(pz-lastz)<2 then stuck=stuck+1 else stuck=0 end
        if stuck>=STUCK_LIM then
            release_all(); stuck=0; stuck_runs=stuck_runs+1
            if stuck_runs>=2 then stuck_runs=0; phase="RECAL"; recal_i=0; return  -- camera likely rotated: recalibrate
            elseif not replan(base) then phase="INIT" end
        end
        lastx,lastz=px,pz
        if (frame%600)==0 then log(string.format("[f%d] wander tile=(%d,%d) steps~%d mode=0x%02X", frame, cc,cr, steps, ru8(GM) or 0)) end
        return
    end
end)

log("s5 encounter armed")
