-- autorun_btnmap.lua  (throwaway)
-- Discover the mapping from PCSX pad button index -> the bit it sets in the
-- per-frame button mask 0x8007B850, by forcing each button alone and reading the
-- mask. Needed so the input recorder/replayer can round-trip: record the mask,
-- replay by pad.force-ing the buttons whose bits are in the recorded mask.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env=require("probe.env"); local mem=require("probe.mem"); local pad=require("probe.pad")
local sstate=require("probe.sstate"); local bp=require("probe.bp")
local FIELD_BP=0x8001698C; local HELD=0x8007B850
local START_SAVE=env.getenv("LEGAIA_SSTATE",""); local OUT_DIR=env.getenv("LEGAIA_OUT_DIR","captures/btnmap")
os.execute(string.format("mkdir -p %q",OUT_DIR)); local LOG=io.open(OUT_DIR.."/btnmap.log","w")
local function log(s) PCSX.log("[bm] "..s); if LOG then LOG:write(s.."\n"); LOG:flush() end end
local function ru16(a) return mem.in_ram(a) and mem.read_u16(a) or 0 end
local NAMES={[0]="SELECT",[1]="L3",[2]="R3",[3]="START",[4]="UP",[5]="RIGHT",[6]="DOWN",[7]="LEFT",[8]="L2",[9]="R2",[10]="L1",[11]="R1",[12]="TRIANGLE",[13]="CIRCLE",[14]="CROSS",[15]="SQUARE"}
local vsync,loaded=0,false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync=vsync+1
    if not loaded and START_SAVE~="" and vsync>=2 then loaded=true; log(sstate.load(START_SAVE) and "resumed" or "FAILED") end
end)
local frame=-1; local btn=0; local phase_start=nil; local HOLD=10; local GAP=6
bp.arm(FIELD_BP,"Exec",4,"ft",function()
    if not loaded then return end
    frame=frame+1
    if frame<30 then return end
    if phase_start==nil then phase_start=frame end
    local cyc=HOLD+GAP; local el=frame-phase_start; local idx=math.floor(el/cyc); local ph=el%cyc
    if idx>15 then log("=== btnmap done ==="); if LOG then LOG:close() end; PCSX.quit(0); return end
    local b=idx
    if ph==0 then pad.force(b)
    elseif ph==HOLD-1 then local m=ru16(HELD); log(string.format("button %2d %-9s -> mask 0x%04X", b, NAMES[b] or "?", m)); pad.release(b) end
end)
log("btnmap armed")
