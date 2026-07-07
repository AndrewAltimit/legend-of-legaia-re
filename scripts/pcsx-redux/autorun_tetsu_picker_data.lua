-- autorun_tetsu_picker_data.lua
--
-- Find the spar menu's option-data source. Replays the recorded inputs to the
-- 4-option list (cursor at *(0x801C6EA4)+0x0C), then - while the menu is up -
-- dumps the talked-to actor's dialogue buffer around the parked script PC.
-- The dialog pager renders text from `*(actor+0x90) + (i16)*(actor+0x9E)`
-- (picker.rs doc), and the menu parks the actor at PC ~0x155, so the picker
-- structure (open byte + N option entries + label segments) lives at
-- dialogue_base + ~0x155. Dumps a hex window there so we can identify the open
-- byte + format and see why `scan_pickers` misses it in the engine's extraction.
--
-- Env: LEGAIA_SSTATE, LEGAIA_INPUTS, LEGAIA_OUT_DIR, LEGAIA_STOP_FRAME.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP = 0x8001698C
local PLAYER   = 0x8007C364
local SCENE_PTR= 0x801C6EA4
local PAGER_TXT= 0x801F3538          -- pager current text pointer (picker.rs)

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local INPUTS     = env.getenv("LEGAIA_INPUTS", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/tetsu_picker_data")
local STOP_FRAME = tonumber(env.getenv("LEGAIA_STOP_FRAME", "1320")) or 1320
local HOLD       = tonumber(env.getenv("LEGAIA_HOLD", "60")) or 60
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/picker.log", "w")
local function log(s) PCSX.log("[tp] "..s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or 0 end
local function ru16(a) return mem.in_ram(a) and mem.read_u16(a) or 0 end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or 0 end
local function s16(v) if v>=0x8000 then return v-0x10000 end; return v end
local function btn_bit(b) if b<8 then return 2^(b+8) else return 2^(b-8) end end

local timeline={}
do local fh=io.open(INPUTS,"r"); if fh then for line in fh:lines() do
    if line:sub(1,1)~="#" and #line>0 then local f,h=line:match("^(%d+),0[xX](%x+)"); if f and h then timeline[#timeline+1]={tonumber(f),tonumber(h,16)} end end
end fh:close() end; log(("loaded %d transitions"):format(#timeline)) end

local held_now={}
local function apply_mask(mask) for b=0,15 do local on=(math.floor(mask/btn_bit(b))%2)==1
    if on and not held_now[b] then pad.force(b); held_now[b]=true elseif (not on) and held_now[b] then pad.release(b); held_now[b]=false end end end

local function hexdump(base, lo, hi)
    for off=lo,hi-1,16 do
        local parts={}
        for i=0,15 do parts[#parts+1]=string.format("%02X", ru8(base+off+i)) end
        log(string.format("  +0x%03X: %s", off, table.concat(parts," ")))
    end
end

local st={frame=-1, ti=1, cur=0, phase="REPLAY", t=nil, dumped=false}
local vsync,loaded=0,false
-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync=vsync+1
    if not loaded and START_SAVE~="" and vsync>=START_DELAY then loaded=true; log(sstate.load(START_SAVE) and "resumed" or "FAILED") end
end)

bp.arm(FIELD_BP,"Exec",4,"ft",function()
    if not loaded then return end
    st.frame=st.frame+1

    if st.phase=="REPLAY" then
        while st.ti<=#timeline and timeline[st.ti][1]<=st.frame do st.cur=timeline[st.ti][2]; st.ti=st.ti+1 end
        apply_mask(st.cur)
        if st.frame>=STOP_FRAME then apply_mask(0); st.phase="DUMP"; st.t=st.frame end
        return
    end

    if st.phase=="DUMP" then
        if st.frame-st.t < 8 then return end   -- let the menu settle
        if st.dumped then
            if st.frame-st.t > HOLD then if LOG then LOG:close() end; PCSX.quit(0) end
            return
        end
        st.dumped=true
        local pp=ru32(PLAYER); local it=pp~=0 and ru32(pp+0x98) or 0
        local sp=ru32(SCENE_PTR)
        local dbase = it~=0 and ru32(it+0x90) or 0
        local dpc   = it~=0 and ru16(it+0x9E) or 0   -- raw u16 script PC
        local pager = ru32(PAGER_TXT)
        log(string.format("interact_actor=0x%08X  actor+0x90(dlg_base)=0x%08X  actor+0x9E(pc)=0x%04X  cursor(+0xC)=0x%02X  pager_txt(0x801F3538)=0x%08X",
            it, dbase, dpc, sp~=0 and ru8(sp+0x0C) or 0, pager))
        if dbase~=0 then
            log(string.format("=== dialogue buffer 0x%08X around pc 0x%X ===", dbase, dpc))
            hexdump(dbase, 0x120, 0x1E0)
        end
        if pager~=0 and pager~=dbase then
            log(string.format("=== pager text pointer 0x%08X region ===", pager))
            hexdump(pager - 0x20, 0, 0x60)
        end
        return
    end
end)
log("tetsu picker-data probe armed")
