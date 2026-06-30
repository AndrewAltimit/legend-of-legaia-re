-- autorun_tetsu_confirm.lua
--
-- Confirm the spar menu mechanism: replay the recorded inputs up to the 4-option
-- list (cursor at index 0), then drive the cursor (scene-ctrl *0x801C6EA4 +0x0C)
-- DOWN to index 2 (the "training fight" entry, the user's 3rd option) and press
-- CROSS - asserting the battle starts (game_mode 0x8007B83C == 0x15). This pins:
--   (a) the menu cursor is *0x801C6EA4 + 0x0C,
--   (b) confirming index 2 starts the spar,
--   (c) the menu is the dialog-SM inline picker, not the field VM.
--
-- Env: LEGAIA_SSTATE, LEGAIA_INPUTS, LEGAIA_OUT_DIR, LEGAIA_STOP_FRAME.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP = 0x8001698C
local GM       = 0x8007B83C
local SCENE_PTR= 0x801C6EA4
local PLAYER   = 0x8007C364

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local INPUTS     = env.getenv("LEGAIA_INPUTS", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/tetsu_confirm")
local STOP_FRAME = tonumber(env.getenv("LEGAIA_STOP_FRAME", "1320")) or 1320
local TARGET_IDX = tonumber(env.getenv("LEGAIA_TARGET_IDX", "2")) or 2
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/confirm.log", "w")
local function log(s) PCSX.log("[tc] "..s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru8(a)  return mem.in_ram(a) and mem.read_u8(a) or 0 end
local function ru32(a) return mem.in_ram(a) and mem.read_u32(a) or 0 end
local function btn_bit(b) if b<8 then return 2^(b+8) else return 2^(b-8) end end
local function cursor() local sp=ru32(SCENE_PTR); return sp~=0 and ru8(sp+0x0C) or 0xFF end

local timeline = {}
do local fh=io.open(INPUTS,"r"); if fh then for line in fh:lines() do
    if line:sub(1,1)~="#" and #line>0 then local f,h=line:match("^(%d+),0[xX](%x+)"); if f and h then timeline[#timeline+1]={tonumber(f),tonumber(h,16)} end end
end fh:close() end; log(("loaded %d transitions"):format(#timeline)) end

local held_now={}
local function apply_mask(mask) for b=0,15 do local on=(math.floor(mask/btn_bit(b))%2)==1
    if on and not held_now[b] then pad.force(b); held_now[b]=true elseif (not on) and held_now[b] then pad.release(b); held_now[b]=false end end end

local st={frame=-1, ti=1, cur=0, phase="REPLAY", t=nil, step=0, cross=false, done=false}
local vsync,loaded=0,false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync=vsync+1
    if not loaded and START_SAVE~="" and vsync>=START_DELAY then loaded=true; log(sstate.load(START_SAVE) and "resumed" or "FAILED") end
end)

bp.arm(FIELD_BP,"Exec",4,"ft",function()
    if not loaded or st.done then return end
    st.frame=st.frame+1
    local m=ru8(GM)
    if m==0x15 then log(("*** BATTLE (mode 0x15) at f%d after confirming cursor index %d ***"):format(st.frame, TARGET_IDX)); st.done=true; if LOG then LOG:close() end; PCSX.quit(0); return end

    if st.phase=="REPLAY" then
        while st.ti<=#timeline and timeline[st.ti][1]<=st.frame do st.cur=timeline[st.ti][2]; st.ti=st.ti+1 end
        apply_mask(st.cur)
        if st.frame>=STOP_FRAME then apply_mask(0); st.phase="NAV"; st.t=st.frame; log(("reached stop f%d, cursor=%d; navigating to index %d"):format(st.frame, cursor(), TARGET_IDX)) end
        return
    end

    if st.phase=="NAV" then
        -- press DOWN one tap at a time until cursor == TARGET_IDX
        local c=cursor()
        if c>=TARGET_IDX then
            apply_mask(0)
            if st.frame-st.t>10 then log(("cursor at %d; pressing CROSS to confirm"):format(c)); st.phase="CONFIRM"; st.t=st.frame end
            return
        end
        -- tap DOWN: 4 frames down, 8 frames release, repeat
        local ph=(st.frame-st.t)%12
        if ph<4 then pad.force(pad.BTN.DOWN) else pad.release(pad.BTN.DOWN) end
        if (st.frame-st.t)%12==0 then log(("nav: cursor=%d"):format(c)) end
        if st.frame-st.t>200 then log("NAV timeout"); st.done=true; if LOG then LOG:close() end; PCSX.quit(0) end
        return
    end

    if st.phase=="CONFIRM" then
        -- pulse CROSS a few times to advance the post-select confirmation
        local ph=(st.frame-st.t)%14
        if ph<3 then pad.force(pad.BTN.CROSS) else pad.release(pad.BTN.CROSS) end
        if (st.frame-st.t)%70==0 then log(("confirm: cursor=%d mode=0x%02X engaged=%s"):format(cursor(), m, tostring((function() local pp=ru32(PLAYER); return pp~=0 and (math.floor(ru32(pp+0x10)/0x80000)%2==1) end)()))) end
        if st.frame-st.t>700 then log("CONFIRM timeout, no battle"); st.done=true; if LOG then LOG:close() end; PCSX.quit(0) end
        return
    end
end)
log("tetsu confirm armed")
