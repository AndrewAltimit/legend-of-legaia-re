-- autorun_s5_tetsu.lua
--
-- S5 capture attempt: reach the FIRST BATTLE from the s4_rimelm_door_transition
-- exterior anchor. Rim Elm (town01) has NO random encounters (an empty 148-step
-- wander confirmed it), so the first battle is the SCRIPTED Tetsu sparring
-- tutorial (formation_id 4), started by talking to the sparring partner. This
-- probe navigates the player to Tetsu's tutorial position - world (2752,1856) =
-- tile (21,14), pinned by the rimelm_npc_press_tetsu capture - over the grid-BFS
-- nav (closest reachable tile to him; explore via doors if he is in another
-- sub-area), then faces him and pulses CROSS to start the spar. A battle =
-- game_mode 0x8007B83C == 0x15 OR battle-context 0x8007BD24 != 0; on the first
-- one it settles and checkpoints. Faithful: D-pad + interact only, no pokes.
--
-- State is bundled in `st` to stay under Lua's 60-upvalue-per-function limit.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_CKPT_LABEL, LEGAIA_MAX_FRAMES,
--      LEGAIA_SETTLE, LEGAIA_STUCK.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP=0x8001698C; local PLAYER=0x8007C364; local SCENE_NAME=0x8007050C
local GM=0x8007B83C; local BATTLE_CTX=0x8007BD24
local ACTOR_TBL=0x801C93C8; local ACTOR_CNT=0x8007B6B8
local FIELDBUF_P=0x1F8003EC; local GRID_OFF=0x4000; local TILE=128; local BATTLE_MODE=0x15
local TETSU_X,TETSU_Z=2752,1856
local TETSU_C,TETSU_R=math.floor(TETSU_X/128),math.floor(TETSU_Z/128)

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s5_tetsu")
local CKPT_LABEL = env.getenv("LEGAIA_CKPT_LABEL", "s5_tetsu_battle")
local SETTLE     = tonumber(env.getenv("LEGAIA_SETTLE", "20")) or 20
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "9000")) or 9000
local STUCK_LIM  = tonumber(env.getenv("LEGAIA_STUCK", "45")) or 45
local SETTLE0    = tonumber(env.getenv("LEGAIA_SETTLE0", "45")) or 45
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2
local WARP_JUMP  = tonumber(env.getenv("LEGAIA_WARP_JUMP", "300")) or 300
local RECAL_HOLD = 14

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/s5.log", "w")
local function log(s) PCSX.log("[s5] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru16(a) return mem.in_ram(a) and mem.read_u16(a) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end
local function s16(v)  if v==nil then return nil end; if v>=0x8000 then return v-0x10000 end; return v end
local function read_scene()
    local s={}; for i=0,7 do local b=ru8(SCENE_NAME+i) or 0; if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end
local function ppos()
    local pp=ru32(PLAYER); if pp==nil then return nil end
    return s16(ru16(pp+0x14)), s16(ru16(pp+0x18))
end
local function engaged()
    local pp=ru32(PLAYER); if pp==nil then return false end
    local fl=ru32(pp+0x10); if fl==nil then return false end
    return math.floor(fl/0x80000)%2==1
end
local function in_battle()
    local m=ru8(GM) or 0; local bc=ru32(BATTLE_CTX) or 0
    return (m==BATTLE_MODE) or (bc~=0), m, bc
end
local function gridbase()
    local b=mem.read_scratch_u32(FIELDBUF_P); if b==nil or b==0 then return nil end; return b
end
local function grid_byte(base,c,r)
    if c<0 or c>=0x80 or r<0 or r>=0x80 then return nil end
    return ru8(base+GRID_OFF+r*0x80+c)
end
local function tile_walkable(base,c,r)
    local b=grid_byte(base,c,r); if b==nil then return false end; return math.floor(b/16)~=0xF
end
local function tile_of(x,z) return math.floor(x/TILE), math.floor(z/TILE) end
local function tile_center(c,r) return c*TILE+TILE/2, r*TILE+TILE/2 end
local function write_checkpoint(label)
    local ok=pcall(function()
        local w=PCSX.createSaveState()
        local fh=Support.File.open(OUT_DIR.."/"..label..".rawsstate","CREATE"); fh:writeMoveSlice(w); fh:close()
        log("checkpoint written: "..OUT_DIR.."/"..label..".rawsstate")
    end)
    if not ok then log("checkpoint FAILED") end
end

-- pad<->world model
local est = { UP={dx=0,dz=1}, DOWN={dx=0,dz=-1}, LEFT={dx=-1,dz=0}, RIGHT={dx=1,dz=0} }
local DIRS = { "UP","DOWN","LEFT","RIGHT" }
local function unit(dx,dz) local m=math.sqrt(dx*dx+dz*dz); if m<1e-6 then return 0,0 end; return dx/m,dz/m end
local function best_button(wx,wz)
    local ux,uz=unit(wx,wz); local bd,bb=-2,"RIGHT"
    for _,b in ipairs(DIRS) do local ex,ez=unit(est[b].dx,est[b].dz); local d=ex*ux+ez*uz; if d>bd then bd=d;bb=b end end
    return bb
end
local function update_est(btn,dx,dz)
    if btn==nil or math.abs(dx)+math.abs(dz)<6 then return end
    local e=est[btn]; e.dx=0.7*e.dx+0.3*dx; e.dz=0.7*e.dz+0.3*dz
end

-- BFS
local function key(c,r) return r*0x80+c end
local function bfs(base,sc,sr)
    local prev,dist,order={},{},{}
    local q={{sc,sr}}; dist[key(sc,sr)]=0; local head=1
    while head<=#q do
        local cur=q[head]; head=head+1; local c,r=cur[1],cur[2]; order[#order+1]={c,r}
        for _,n in ipairs({ {c+1,r},{c-1,r},{c,r+1},{c,r-1} }) do
            local nc,nr=n[1],n[2]; local k=key(nc,nr)
            if dist[k]==nil and tile_walkable(base,nc,nr) then dist[k]=dist[key(c,r)]+1; prev[k]={c,r}; q[#q+1]={nc,nr} end
        end
    end
    return prev,dist,order
end
local function path_to(prev,sc,sr,tc,tr)
    local p={}; local c,r=tc,tr
    while not (c==sc and r==sr) do p[#p+1]={c,r}; local pr=prev[key(c,r)]; if pr==nil then return nil end; c,r=pr[1],pr[2] end
    local out={}; for i=#p,1,-1 do out[#out+1]=p[i] end; return out
end

local visits={}
local function pick_tetsu(order)
    local best,bd=nil,1e9
    for _,t in ipairs(order) do local c,r=t[1],t[2]; local d=math.abs(c-TETSU_C)+math.abs(r-TETSU_R); if d<bd then bd=d;best={c,r} end end
    return best,bd
end
local function pick_wander(dist,order)
    local best,bs=nil,-1e18
    for _,t in ipairs(order) do local c,r=t[1],t[2]; local sc=(dist[key(c,r)] or 0)-(visits[key(c,r)] or 0)*40; if sc>bs then bs=sc;best={c,r} end end
    return best
end

-- shared mutable state (one upvalue keeps the closures under the 60 limit)
local st = {
    frame=0, phase="INIT", base=nil, path=nil, pi=1, cur_btn=nil,
    lastx=nil, lastz=nil, stuck=0, stuck_runs=0, cross_state=0, cross_t=0,
    steps=0, was_engaged=false, last_tilekey=nil, goal_dist=1e9,
    recal_i=0, recal_start=nil, recal_x0=nil, recal_z0=nil,
    talk_start=nil, tetsu_dumped=false,
    battle_seen=false, done=false, cap_since=nil, vsync=0, loaded=false,
}

local held={}
local function release_all() for _,b in ipairs(DIRS) do if held[b] then pad.release(pad.BTN[b]); held[b]=nil end end end
local function hold_only(btn)
    for _,b in ipairs(DIRS) do
        if b==btn then if not held[b] then pad.force(pad.BTN[b]); held[b]=true end
        else if held[b] then pad.release(pad.BTN[b]); held[b]=false end end
    end
end

local function replan()
    local x,z=ppos(); if x==nil then return false end
    local sc,sr=tile_of(x,z)
    local prev,dist,order=bfs(st.base,sc,sr)
    local tgt,td=pick_tetsu(order); st.goal_dist=td or 1e9
    if tgt==nil then return false end
    if td and td>2 then tgt=pick_wander(dist,order) end
    if tgt==nil then return false end
    st.path=path_to(prev,sc,sr,tgt[1],tgt[2]); st.pi=1
    return st.path~=nil and #st.path>0
end

PCSX.Events.createEventListener("GPU::Vsync", function()
    st.vsync=st.vsync+1
    if not st.loaded and START_SAVE~="" and st.vsync>=START_DELAY then
        st.loaded=true
        log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or ("FAILED "..START_SAVE)); return
    end
    if not st.loaded or st.done then return end
    local b,m,bc=in_battle()
    if b then
        if not st.battle_seen then st.battle_seen=true
            log(string.format("*** [v%d] BATTLE detected mode=0x%02X battle_ctx=0x%08X steps~%d ***", st.vsync,m,bc,st.steps)) end
        if st.cap_since==nil then st.cap_since=st.vsync
        elseif st.vsync-st.cap_since>=SETTLE then
            log(string.format("[v%d] battle settled (mode=0x%02X ctx=0x%08X); checkpointing", st.vsync,m,bc))
            write_checkpoint(CKPT_LABEL); st.done=true
            log("done; quitting"); if LOG then LOG:close() end; PCSX.quit(0)
        end
    else st.cap_since=nil end
end)

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    if st.done then return end
    st.frame=st.frame+1
    if st.frame<=SETTLE0 then return end
    if st.frame>=MAX_FRAMES then release_all(); log(string.format("MAX_FRAMES, no battle. scene=%q steps~%d goal_dist=%d", read_scene(), st.steps, st.goal_dist)); if LOG then LOG:close() end; PCSX.quit(0); return end
    if st.battle_seen or in_battle() then release_all(); return end

    local x,z=ppos()

    if st.phase=="INIT" then
        st.base=gridbase(); if st.base==nil then return end
        if x==nil then return end
        local pc,pr=tile_of(x,z)
        log(string.format("INIT scene=%q mode=0x%02X player tile (%d,%d) Tetsu tile (%d,%d)", read_scene(), ru8(GM) or 0, pc,pr, TETSU_C,TETSU_R))
        if not replan() then log("INIT replan failed"); return end
        st.phase="WANDER"; st.lastx,st.lastz=x,z; st.last_tilekey=key(pc,pr); return
    end

    local eng=engaged()
    if eng~=st.was_engaged then
        local pp=ru32(PLAYER) or 0; local it=ru32(pp+0x98) or 0
        log(string.format("[f%d] engaged %s->%s interact=0x%08X tile=(%s,%s)", st.frame, tostring(st.was_engaged), tostring(eng), it, tostring(x and math.floor(x/TILE)), tostring(z and math.floor(z/TILE))))
        st.was_engaged=eng
    end
    if eng then
        release_all()
        if st.cross_state==1 and st.frame>=st.cross_t then pad.release(pad.BTN.CROSS); st.cross_state=0; st.cross_t=st.frame+6
        elseif st.cross_state==0 and st.frame>=st.cross_t then pad.force(pad.BTN.CROSS); st.cross_state=1; st.cross_t=st.frame+3 end
        return
    end

    if st.phase=="RECAL" then
        if st.recal_i==0 then st.recal_i=1; st.recal_start=st.frame; st.recal_x0,st.recal_z0=x,z; hold_only(DIRS[1]); return end
        local d=DIRS[st.recal_i]
        if st.frame-st.recal_start>=RECAL_HOLD then
            local dx=(x or 0)-(st.recal_x0 or 0); local dz=(z or 0)-(st.recal_z0 or 0)
            if math.abs(dx)+math.abs(dz)>=4 then est[d]={dx=dx,dz=dz} end
            st.recal_i=st.recal_i+1
            if st.recal_i>#DIRS then
                release_all(); st.recal_i=0
                log(string.format("[f%d] RECAL UP(%d,%d) DOWN(%d,%d) LEFT(%d,%d) RIGHT(%d,%d)", st.frame, est.UP.dx,est.UP.dz, est.DOWN.dx,est.DOWN.dz, est.LEFT.dx,est.LEFT.dz, est.RIGHT.dx,est.RIGHT.dz))
                if not replan() then st.phase="INIT" else st.phase="WANDER" end
                st.stuck=0; st.lastx,st.lastz=x,z
            else st.recal_start=st.frame; st.recal_x0,st.recal_z0=x,z; hold_only(DIRS[st.recal_i]) end
        end
        return
    end

    if st.phase=="WANDER" then
        if x then local tk=key(tile_of(x,z)); if tk~=st.last_tilekey then st.steps=st.steps+1; visits[tk]=(visits[tk] or 0)+1; st.last_tilekey=tk end end
        if x and st.lastx and (math.abs(x-st.lastx)+math.abs(z-st.lastz))>WARP_JUMP then
            log(string.format("[f%d] warp (%d,%d)->(%d,%d); RECAL", st.frame, st.lastx,st.lastz,x,z))
            release_all(); st.lastx,st.lastz=x,z; st.stuck=0; st.stuck_runs=0; st.phase="RECAL"; st.recal_i=0; return
        end
        if st.cross_state==1 and st.frame>=st.cross_t then pad.release(pad.BTN.CROSS); st.cross_state=0; st.cross_t=st.frame+10
        elseif st.cross_state==0 and st.frame>=st.cross_t then pad.force(pad.BTN.CROSS); st.cross_state=1; st.cross_t=st.frame+2 end
        if st.cur_btn and st.lastx then update_est(st.cur_btn, x-st.lastx, z-st.lastz) end

        if st.path==nil or st.pi>#st.path then
            if st.goal_dist<=2 then st.phase="TALK"; st.talk_start=st.frame; return end
            if not replan() then st.phase="INIT" end; return
        end
        local cc,cr=tile_of(x,z)
        while st.pi<=#st.path and cc==st.path[st.pi][1] and cr==st.path[st.pi][2] do st.pi=st.pi+1 end
        if st.pi>#st.path then
            if st.goal_dist<=2 then st.phase="TALK"; st.talk_start=st.frame; return end
            if not replan() then st.phase="INIT" end; return
        end
        local tc,tr=st.path[st.pi][1],st.path[st.pi][2]
        local wx,wz=tile_center(tc,tr)
        local btn=best_button(wx-x, wz-z); hold_only(btn); st.cur_btn=btn
        if st.lastx and math.abs(x-st.lastx)+math.abs(z-st.lastz)<2 then st.stuck=st.stuck+1 else st.stuck=0 end
        if st.stuck>=STUCK_LIM then
            release_all(); st.stuck=0; st.stuck_runs=st.stuck_runs+1
            if st.stuck_runs>=2 then st.stuck_runs=0; st.phase="RECAL"; st.recal_i=0; return
            elseif not replan() then st.phase="INIT" end
        end
        st.lastx,st.lastz=x,z
        if (st.frame%600)==0 then log(string.format("[f%d] wander tile=(%d,%d) steps~%d goal_dist=%d", st.frame, cc,cr, st.steps, st.goal_dist)) end
        return
    end

    if st.phase=="TALK" then
        if not st.tetsu_dumped then
            st.tetsu_dumped=true
            local cnt=ru8(ACTOR_CNT) or 0
            log(string.format("[f%d] at Tetsu approach goal_dist=%d player_tile=(%s,%s) actors=%d:", st.frame, st.goal_dist, tostring(x and math.floor(x/TILE)), tostring(z and math.floor(z/TILE)), cnt))
            for i=0,(cnt>0 and cnt-1 or 0) do
                local ap=ru32(ACTOR_TBL+i*4) or 0
                if ap~=0 and mem.in_ram(ap+0x60) then
                    local ax,az=s16(ru16(ap+0x14)),s16(ru16(ap+0x18)); local fl=ru32(ap+0x10) or 0
                    log(string.format("    [%d] @0x%08X pos=(%s,%s) tile=(%s,%s) flags=0x%08X", i, ap, tostring(ax),tostring(az), tostring(ax and math.floor(ax/TILE)),tostring(az and math.floor(az/TILE)), fl))
                end
            end
        end
        local btn=best_button(TETSU_X-(x or 0), TETSU_Z-(z or 0)); hold_only(btn)
        if st.cross_state==1 and st.frame>=st.cross_t then pad.release(pad.BTN.CROSS); st.cross_state=0; st.cross_t=st.frame+4
        elseif st.cross_state==0 and st.frame>=st.cross_t then pad.force(pad.BTN.CROSS); st.cross_state=1; st.cross_t=st.frame+3 end
        if st.talk_start and st.frame-st.talk_start>400 then
            release_all(); st.tetsu_dumped=false; visits[key(TETSU_C,TETSU_R)]=(visits[key(TETSU_C,TETSU_R)] or 0)+100
            log(string.format("[f%d] TALK timeout; exploring", st.frame))
            if not replan() then st.phase="INIT" else st.phase="WANDER" end
            st.stuck=0; st.stuck_runs=0; st.lastx,st.lastz=x,z
        end
        return
    end
end)

log("s5 tetsu nav armed")
