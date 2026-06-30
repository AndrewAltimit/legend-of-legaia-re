-- autorun_s4_navsweep.lua
--
-- Deterministic S4 door navigation: a self-calibrating world-space coverage
-- sweep. The field controller remaps the held pad by the camera, so this first
-- CALIBRATES (moves into open space, then measures each pad direction's net
-- (dX,dZ) displacement in player+0x14/+0x18) to learn which pad button drives
-- +X / -X / +Z / -Z in world space. It then SWEEPS a serpentine over the
-- walkable area (run a world axis until blocked, step the other axis, reverse),
-- pulsing CROSS throughout to interact with any door/NPC it passes (faithful:
-- it is just walking + pressing the interact button). A scene-name change =
-- transition -> checkpoint. NPC dialogue is dismissed with CROSS.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_CKPT_LABEL, LEGAIA_SETTLE,
--      LEGAIA_MAX_FRAMES, LEGAIA_CAL_DIR (open-space prime direction).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP   = 0x8001698C
local GM         = 0x8007B83C
local SCENE_NAME = 0x8007050C
local PLAYER     = 0x8007C364

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/s4_nav")
local CKPT_LABEL = env.getenv("LEGAIA_CKPT_LABEL", "s4_transition")
local HOME_SCENE = env.getenv("LEGAIA_HOME_SCENE", "town01")
local SETTLE     = tonumber(env.getenv("LEGAIA_SETTLE", "20")) or 20
local MAX_FRAMES = tonumber(env.getenv("LEGAIA_MAX_FRAMES", "6000")) or 6000
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2
local CAL_DIR    = env.getenv("LEGAIA_CAL_DIR", "DOWN") -- open-space prime dir

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/nav.log", "w")
local function log(s) PCSX.log("[nav] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or nil end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or nil end
local function s32(v)  if v == nil then return nil end; if v >= 0x80000000 then return v - 0x100000000 end; return v end
local function read_scene()
    local s = {}
    for i=0,7 do local b=ru8(SCENE_NAME+i) or 0; if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end
local function engaged()
    local pp=ru32(PLAYER); if pp==nil then return nil end
    local fl=ru32(pp+0x10); if fl==nil then return nil end
    return math.floor(fl/0x80000)%2==1
end
local function ppos()
    local pp=ru32(PLAYER); if pp==nil then return nil end
    return s32(ru32(pp+0x14)), s32(ru32(pp+0x18))
end
local function write_checkpoint(label)
    local ok=pcall(function()
        local w=PCSX.createSaveState()
        local fh=Support.File.open(OUT_DIR.."/"..label..".rawsstate","CREATE"); fh:writeMoveSlice(w); fh:close()
        log("checkpoint written: "..OUT_DIR.."/"..label..".rawsstate")
    end)
    if not ok then log("checkpoint FAILED") end
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
local held = nil
local function set_dir(d)
    if held and held~=d then pad.release(pad.BTN[held]) end
    if d and held~=d then pad.force(pad.BTN[d]) end
    held=d
end

-- learned world-axis -> pad direction (set in calibration)
local AX = {}  -- AX.xp / AX.xm / AX.zp / AX.zm = pad dir names
local frame = 0
local phase = "PRIME"      -- PRIME -> CAL -> SWEEP -> SETTLE_NEW -> DONE
local prime_end = 220
local cal = { i = 0, samples = {}, hold = 50, gap = 12, start = nil }
-- sweep state
local sweep_axis = "x"     -- run along x then step z
local sweep_x_sign = 1
local sweep_z_sign = 1
local run_dir, step_dir = nil, nil
local last_x, last_y, stuck = nil, nil, 0
local cross_until, cross_cd = 0, 0
local stepping = 0
local new_scene, target_since, quit_at = nil, nil, nil

local function pick_axes()
    -- from cal.samples[dir] = {dx,dz}, choose the pad dir maximizing each axis
    local best = { xp={-1e18,nil}, xm={1e18,nil}, zp={-1e18,nil}, zm={1e18,nil} }
    for _,d in ipairs(DIRS) do
        local s = cal.samples[d]; if s then
            if s.dx > best.xp[1] then best.xp = {s.dx,d} end
            if s.dx < best.xm[1] then best.xm = {s.dx,d} end
            if s.dz > best.zp[1] then best.zp = {s.dz,d} end
            if s.dz < best.zm[1] then best.zm = {s.dz,d} end
        end
    end
    AX.xp,AX.xm,AX.zp,AX.zm = best.xp[2],best.xm[2],best.zp[2],best.zm[2]
    log(string.format("axes: +X=%s -X=%s +Z=%s -Z=%s", tostring(AX.xp),tostring(AX.xm),tostring(AX.zp),tostring(AX.zm)))
end

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame=frame+1
    if frame>=MAX_FRAMES then if held then pad.release(pad.BTN[held]) end
        log(string.format("MAX_FRAMES (%d), scene=%q", MAX_FRAMES, read_scene()))
        if LOG then LOG:close() end; PCSX.quit(0) end

    local sc = read_scene()
    if phase~="SETTLE_NEW" and phase~="DONE" and sc~=HOME_SCENE and sc~="" then
        if held then pad.release(pad.BTN[held]); held=nil end
        new_scene=sc; log(string.format("*** [f%d] TRANSITION town01 -> %q (mode=0x%02X) ***",frame,sc,ru8(GM) or 0xFF))
        phase="SETTLE_NEW"; return
    end

    if phase=="PRIME" then
        set_dir(CAL_DIR)  -- move into open space
        if frame>=prime_end then set_dir(nil); phase="CAL"; cal.start=frame; log("phase -> CAL") end
        return
    end

    if phase=="CAL" then
        local cyc=cal.hold+cal.gap
        local el=frame-cal.start
        local idx=math.floor(el/cyc)+1
        if idx>#DIRS then set_dir(nil); pick_axes(); phase="SWEEP"
            run_dir=(sweep_x_sign>0) and AX.xp or AX.xm
            step_dir=(sweep_z_sign>0) and AX.zp or AX.zm
            log("phase -> SWEEP"); return end
        local ph=el%cyc; local d=DIRS[idx]
        if ph==0 then local x,z=ppos(); cal.samples[d]={x0=x,z0=z}; set_dir(d)
        elseif ph==cal.hold then set_dir(nil); local x,z=ppos(); local s=cal.samples[d]
            s.dx=(x or 0)-(s.x0 or 0); s.dz=(z or 0)-(s.z0 or 0)
            log(string.format("cal %-6s dX=%d dZ=%d",d,s.dx,s.dz)) end
        return
    end

    if phase=="SWEEP" then
        -- dismiss dialogue
        if engaged() then
            if cross_until>0 and frame>=cross_until then pad.release(pad.BTN.CROSS); cross_until=0; cross_cd=frame+8
            elseif cross_until==0 and frame>=cross_cd then pad.force(pad.BTN.CROSS); cross_until=frame+3 end
            return
        end
        -- pulse CROSS (interact) while moving to catch passed doors
        if cross_until>0 and frame>=cross_until then pad.release(pad.BTN.CROSS); cross_until=0; cross_cd=frame+10
        elseif cross_until==0 and frame>=cross_cd and stepping==0 then pad.force(pad.BTN.CROSS); cross_until=frame+2 end

        local x,y=ppos()
        if stepping>0 then
            set_dir(step_dir); stepping=stepping-1
            if stepping==0 then
                -- after stepping z: if no progress vs before step, z exhausted -> reverse z, flip x dir
                sweep_x_sign=-sweep_x_sign; run_dir=(sweep_x_sign>0) and AX.xp or AX.xm
                last_x,last_y,stuck=x,y,0
            end
            last_x,last_y=x,y; return
        end
        set_dir(run_dir)
        if last_x~=nil and x==last_x and y==last_y then stuck=stuck+1 else stuck=0 end
        last_x,last_y=x,y
        if stuck>=20 then
            -- blocked at end of an X run: step along Z
            stuck=0; stepping=18
            local z_before = y
            -- if we keep failing to make Z progress, flip Z sign
        end
        if (frame%400)==0 then log(string.format("[f%d] sweep run=%s step=%s pos=(%d,%d)",frame,tostring(run_dir),tostring(step_dir),x or 0,y or 0)) end
        return
    end

    if phase=="SETTLE_NEW" then
        local m=ru8(GM) or 0xFF
        if m==0x03 and sc==new_scene then
            if target_since==nil then target_since=frame
            elseif frame-target_since>=SETTLE then
                log(string.format("[f%d] settled in %q; checkpointing",frame,new_scene))
                write_checkpoint(CKPT_LABEL); phase="DONE"; quit_at=frame+2 end
        else target_since=nil end
        return
    end

    if phase=="DONE" and quit_at and frame>=quit_at then if LOG then LOG:close() end; PCSX.quit(0) end
end)

log("s4 navsweep armed")
