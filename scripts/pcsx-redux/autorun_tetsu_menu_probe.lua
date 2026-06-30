-- autorun_tetsu_menu_probe.lua
--
-- Crack the Rim Elm spar's 4-option menu mechanism. Replays the recorded s5
-- inputs (the human playthrough) up to just before the menu cursor is moved (the
-- user's DOWN presses are at frames ~1332/1345), then HOLDS neutral so the
-- 4-option list stays on screen, and:
--   1. histograms the field-VM dispatcher FUN_801DE840 (a0=record_base, a1=pc,
--      a2=ctx) - if the menu is field-VM-driven, the dispatcher re-enters at a
--      stable (base, pc) and the opcode at base+pc IS the menu op;
--   2. logs the engaged actor's script PC (+0x9E) + sub-state (+0x50) and the
--      field-control block (*0x801C6EA4 +0x0C cursor / +0x60 interact / +0x62);
--   3. injects DOWN for a few frames and diffs the scene-control + engaged-actor
--      blocks before/after to locate the menu CURSOR variable.
--
-- Env: LEGAIA_SSTATE, LEGAIA_INPUTS, LEGAIA_OUT_DIR, LEGAIA_STOP_FRAME,
--      LEGAIA_PROBE_FRAMES.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP = 0x8001698C
local FIELD_VM = 0x801DE840          -- dispatcher: a0=base, a1=pc, a2=ctx
local PLAYER   = 0x8007C364
local SCENE_PTR= 0x801C6EA4

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local INPUTS     = env.getenv("LEGAIA_INPUTS", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/tetsu_menu")
local STOP_FRAME = tonumber(env.getenv("LEGAIA_STOP_FRAME", "1320")) or 1320
local PROBE_FRAMES = tonumber(env.getenv("LEGAIA_PROBE_FRAMES", "200")) or 200
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/menu.log", "w")
local function log(s) PCSX.log("[tm] "..s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or 0 end
local function ru16(a) return mem.in_ram(a) and mem.read_u16(a) or 0 end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or 0 end
local function btn_bit(b) if b<8 then return 2^(b+8) else return 2^(b-8) end end

-- timeline
local timeline = {}
do
    local fh = io.open(INPUTS, "r")
    if fh then for line in fh:lines() do
        if line:sub(1,1)~="#" and #line>0 then
            local f,h = line:match("^(%d+),0[xX](%x+)")
            if f and h then timeline[#timeline+1]={tonumber(f), tonumber(h,16)} end
        end
    end fh:close() end
    log(("loaded %d transitions from %s"):format(#timeline, INPUTS))
end

local held_now = {}
local function apply_mask(mask)
    for b=0,15 do
        local on = (math.floor(mask/btn_bit(b))%2)==1
        if on and not held_now[b] then pad.force(b); held_now[b]=true
        elseif (not on) and held_now[b] then pad.release(b); held_now[b]=false end
    end
end

local st = { frame=-1, ti=1, cur=0, phase="REPLAY", pstart=nil, dumped=false, down_at=nil }
local hist = {}                 -- "a0:a1" -> {n, op}
local hist_on = false
local snap_before = nil

local function snap_block(base, n)
    local t={}; for i=0,n-1 do t[i]=ru8(base+i) end; return t
end
local function diff_block(before, base, n, tag)
    local changes={}
    for i=0,n-1 do local now=ru8(base+i); if before[i]~=now then changes[#changes+1]=string.format("+0x%X:0x%02X->0x%02X",i,before[i],now) end end
    if #changes>0 then log(("  [%s] changed: %s"):format(tag, table.concat(changes,"  "))) end
    return #changes
end
local function dump_state(tag)
    local pp=ru32(PLAYER); local sp=ru32(SCENE_PTR)
    local it = pp~=0 and ru32(pp+0x98) or 0
    local eng = pp~=0 and (math.floor(ru32(pp+0x10)/0x80000)%2==1) or false
    log(string.format("[%s f%d] engaged=%s interact=0x%08X scenePtr=0x%08X dlg62=0x%02X cur0C=0x%02X int60=0x%02X actor9E=0x%04X actor50=0x%02X",
        tag, st.frame, tostring(eng), it, sp,
        sp~=0 and ru8(sp+0x62) or 0, sp~=0 and ru8(sp+0x0C) or 0, sp~=0 and ru8(sp+0x60) or 0,
        it~=0 and ru16(it+0x9E) or 0, it~=0 and ru8(it+0x50) or 0))
end

local vsync, loaded = 0, false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync=vsync+1
    if not loaded and START_SAVE~="" and vsync>=START_DELAY then
        loaded=true; log(sstate.load(START_SAVE) and ("resumed "..START_SAVE) or "FAILED")
    end
end)

-- field-VM dispatcher histogram (only while hist_on)
bp.arm(FIELD_VM, "Exec", 4, "field_vm", function()
    if not hist_on then return end
    local r = PCSX.getRegisters()
    local a0 = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
    local a1 = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
    local key = string.format("%08X:%X", a0, a1)
    local e = hist[key]
    if e then e.n=e.n+1 else hist[key]={n=1, op=(mem.in_ram(a0+a1) and mem.read_u8(a0+a1) or -1), a0=a0, a1=a1} end
end)

bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    if not loaded then return end
    st.frame=st.frame+1

    if st.phase=="REPLAY" then
        while st.ti<=#timeline and timeline[st.ti][1]<=st.frame do st.cur=timeline[st.ti][2]; st.ti=st.ti+1 end
        apply_mask(st.cur)
        if st.frame>=STOP_FRAME then
            apply_mask(0)   -- release everything; the menu waits for input
            st.phase="PROBE"; st.pstart=st.frame; hist_on=true
            log(string.format("=== reached STOP_FRAME %d; holding, menu should be up ===", STOP_FRAME))
            dump_state("stop")
        end
        return
    end

    if st.phase=="PROBE" then
        local el = st.frame - st.pstart
        if (el%20)==0 then dump_state("hold") end
        -- at el==60 snapshot, el==65 inject DOWN, el==90 diff (cursor finder)
        if el==60 then
            local pp=ru32(PLAYER); local sp=ru32(SCENE_PTR); local it=pp~=0 and ru32(pp+0x98) or 0
            snap_before = { sp=sp, it=it, scene=snap_block(sp, 0x100), actor = it~=0 and snap_block(it,0x100) or nil }
            log("snapshot before DOWN taken")
        elseif el>=65 and el<78 then
            apply_mask(btn_bit(6)*0 + 0)  -- no-op; we use raw pad below
            pad.force(pad.BTN.DOWN)
        elseif el==78 then
            pad.release(pad.BTN.DOWN)
        elseif el==90 and snap_before then
            log("=== cursor diff after DOWN ===")
            diff_block(snap_before.scene, snap_before.sp, 0x100, "sceneCtrl")
            if snap_before.actor then diff_block(snap_before.actor, snap_before.it, 0x100, "engagedActor") end
        end
        if el>=PROBE_FRAMES then
            -- dump histogram top entries
            log("=== FUN_801DE840 (base:pc) histogram while menu up ===")
            local arr={}; for k,v in pairs(hist) do arr[#arr+1]={k=k,v=v} end
            table.sort(arr, function(a,b) return a.v.n>b.v.n end)
            for i=1,math.min(12,#arr) do local e=arr[i]
                log(string.format("  %-18s n=%-5d op=0x%02X (base=0x%08X pc=0x%X)", e.k, e.v.n, e.v.op % 256, e.v.a0, e.v.a1))
            end
            if LOG then LOG:close() end; PCSX.quit(0)
        end
        return
    end
end)

log("tetsu menu probe armed")
