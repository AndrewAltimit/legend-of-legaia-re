-- autorun_seru_blit_probe.lua
--
-- The "It was the Seru." caption during opdeene is NOT ASCII in RAM (any
-- encoding) and NOT drawn by any text renderer (census: balloon spawner,
-- text-actor register, crawl roller, MES renderer, single-line, dialog-glyph
-- emitter all either 0 hits or render only the 22 ASCII crawl pages). That
-- leaves a pre-rendered IMAGE blit. This probe arms the two image/icon sprite
-- drawers plus the MES renderer and, in a narrow window around the caption fade
-- (default rel [800,850]), logs EVERY call with full args + bytes at pointer
-- args. Whatever draws a wide sprite at screen center is the caption; its `desc`
-- pointer's VA pins the source (scene TIM vs SCUS rodata vs loaded asset).
--
--   FUN_8002BDC4  Textured-image blit  (a0=x, a1=y, a2=desc, a3=clut, +w, +h)
--   FUN_8002C488  Icon/glyph drawer    (a0=x, a1=y, a2=icon_id)
--   FUN_80036888  MES text renderer    (a0=buf)
--
-- Cold-boot title driver identical to autorun_crawl1_capture.lua.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env = require("probe.env")
local mem = require("probe.mem")
local pad = require("probe.pad")
local bp  = require("probe.bp")

local GM         = 0x8007B83C
local SCENE_NAME = 0x8007050C
local TITLE_BP   = 0x801DD35C
local FIELD_BP   = 0x8001698C

local IMGBLIT = 0x8002BDC4
local ICON    = 0x8002C488
local MESREND = 0x80036888

local OUT_DIR   = env.getenv("LEGAIA_OUT_DIR", "captures/seru_blit")
local WIN_LO    = tonumber(env.getenv("LEGAIA_WIN_LO", "800")) or 800
local WIN_HI    = tonumber(env.getenv("LEGAIA_WIN_HI", "850")) or 850
local TITLE_MAX = tonumber(env.getenv("LEGAIA_TITLE_MAX", "40000")) or 40000
local MASH_EVERY= tonumber(env.getenv("LEGAIA_MASH_EVERY", "20")) or 20

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/blit.log", "w")
local function log(s) PCSX.log("[blit] " .. s); if LOG then LOG:write(s.."\n"); LOG:flush() end end

local function tou32(v) v=v or 0; if v<0 then return v+0x100000000 end; return v end
local function tos16(v) v=v%0x10000; if v>=0x8000 then v=v-0x10000 end; return v end
local function read_mode() return mem.in_ram(GM) and mem.read_u8(GM) or nil end
local function read_scene()
    if not mem.in_ram(SCENE_NAME) then return "" end
    local s={}
    for i=0,7 do local b=mem.read_u8(SCENE_NAME+i) or 0
        if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end
local function dump(addr,len)
    if not mem.in_ram(addr) then return "(not in RAM)" end
    local hex,asc={},{}
    for i=0,len-1 do local b=mem.read_u8(addr+i) or 0
        hex[#hex+1]=string.format("%02X",b)
        asc[#asc+1]=(b>=0x20 and b<0x7f) and string.char(b) or "." end
    return table.concat(hex," ").."  |"..table.concat(asc).."|"
end

local PHASE="TITLE"
local g_title_tick,g_tick,g_pulse,g_release_at=0,0,0,0
local g_held={}
local opdeene_enter_tick=0
local g_quit_at=nil
local g_counts={IMGBLIT=0,ICON=0,MESREND=0}

local function hold(b) pad.force(b); g_held[#g_held+1]=b end
local function release_all() for _,b in ipairs(g_held) do pad.release(b) end; g_held={} end
local function finish(code,why)
    if PHASE=="DONE" then return end
    PHASE="DONE"; release_all()
    log(string.format("done: %s  IMGBLIT=%d ICON=%d MESREND=%d",
        why, g_counts.IMGBLIT, g_counts.ICON, g_counts.MESREND))
    if LOG then LOG:close() end
    g_quit_at={code=code, at=g_tick+2, title_at=g_title_tick+2}
end

local PATTERN={ {pad.BTN.START}, {pad.BTN.UP}, {pad.BTN.CROSS} }
local function opening_reached() return read_mode()==3 and read_scene()=="opdeene" end
local function enter_capture(from)
    release_all(); PHASE="CAPTURE"; opdeene_enter_tick=g_tick
    log(string.format("CAPTURE start (%s): field=%d win=[%d,%d]", from, g_tick, WIN_LO, WIN_HI))
end

local function in_window() local rel=g_tick-opdeene_enter_tick; return rel>=WIN_LO and rel<=WIN_HI, rel end

local function on_imgblit()
    g_counts.IMGBLIT=g_counts.IMGBLIT+1
    if PHASE~="CAPTURE" then return end
    local ok,rel=in_window(); if not ok then return end
    local n=PCSX.getRegisters().GPR.n
    local x,y=tos16(tou32(n.a0)),tos16(tou32(n.a1))
    local desc,clut=tou32(n.a2),tou32(n.a3)
    log(string.format("IMGBLIT rel=%d x=%d y=%d desc=0x%08X clut=0x%08X ra=0x%08X",
        rel, x, y, desc, clut, tou32(n.ra)))
    log("    desc: "..dump(desc,32))
end
local function on_icon()
    g_counts.ICON=g_counts.ICON+1
    if PHASE~="CAPTURE" then return end
    local ok,rel=in_window(); if not ok then return end
    local n=PCSX.getRegisters().GPR.n
    log(string.format("ICON rel=%d x=%d y=%d icon=0x%X ra=0x%08X",
        rel, tos16(tou32(n.a0)), tos16(tou32(n.a1)), tou32(n.a2), tou32(n.ra)))
end
local function on_mesrend()
    g_counts.MESREND=g_counts.MESREND+1
    if PHASE~="CAPTURE" then return end
    local ok,rel=in_window(); if not ok then return end
    local n=PCSX.getRegisters().GPR.n
    local a0=tou32(n.a0)
    -- FUN_80036888(buf, palette, count, x=a3, y@0x50(sp)); log buf + x.
    log(string.format("MESREND rel=%d a0=0x%08X x=%d ra=0x%08X",
        rel, a0, tos16(tou32(n.a3)), tou32(n.ra)))
    log("    buf: "..dump(a0,32))
end

local function title_tick()
    g_title_tick=g_title_tick+1
    if PHASE=="DONE" then if g_quit_at and g_title_tick>=g_quit_at.title_at then PCSX.quit(g_quit_at.code) end; return end
    if PHASE~="TITLE" then return end
    local scene=read_scene()
    if opening_reached() then enter_capture("title"); return end
    if scene~="opdeene" and scene~="" and read_mode()==3 then finish(1,"wrong path "..scene); return end
    if g_title_tick>=TITLE_MAX then finish(1,"title timeout"); return end
    if g_release_at>0 and g_title_tick>=g_release_at then release_all(); g_release_at=0
    elseif g_release_at==0 and (g_title_tick%MASH_EVERY)==0 then
        g_pulse=g_pulse+1
        for _,b in ipairs(PATTERN[(g_pulse%#PATTERN)+1]) do hold(b) end
        g_release_at=g_title_tick+5
    end
end
local function field_tick()
    g_tick=g_tick+1
    if PHASE=="DONE" then if g_quit_at and g_tick>=g_quit_at.at then PCSX.quit(g_quit_at.code) end; return end
    if PHASE=="TITLE" then if opening_reached() then enter_capture("field") end; return end
    local rel=g_tick-opdeene_enter_tick
    if (g_tick%100)==0 then log(string.format("...rel %d scene=%q", rel, read_scene())) end
    if rel > WIN_HI + 20 then finish(0, "past window"); return end
    if rel >= 3200 then finish(0,"budget") end
end

pcall(function() bp.arm(TITLE_BP,"Exec",4,"title_tick",title_tick) end)
pcall(function() bp.arm(FIELD_BP,"Exec",4,"field_tick",field_tick) end)
pcall(function() bp.arm(IMGBLIT,"Exec",4,"imgblit",on_imgblit) end)
pcall(function() bp.arm(ICON,"Exec",4,"icon",on_icon) end)
pcall(function() bp.arm(MESREND,"Exec",4,"mesrend",on_mesrend) end)
log(string.format("seru blit probe armed: win=[%d,%d] out=%s", WIN_LO, WIN_HI, OUT_DIR))

PCSX.Events.createEventListener("GPU::Vsync", function()
    if PHASE=="DONE" and g_quit_at then
        g_quit_at.vs_seen=(g_quit_at.vs_seen or 0)+1
        if g_quit_at.vs_seen>10 then PCSX.quit(g_quit_at.code) end
    end
end)
