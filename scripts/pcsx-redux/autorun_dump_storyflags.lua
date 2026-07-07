-- autorun_dump_storyflags.lua
--
-- Dump the story-flag bank + key story-progress RAM from a resumed save state,
-- so two states can be diffed to find which scripted beat one has and the other
-- skipped. The field-VM story-flag bitfield base is 0x80085758 (SC offset
-- 0x1618; see docs/subsystems/field-locomotion.md map03 conditional walls). Also
-- dumps the lead character record header and the scene name / mode for context.
--
-- Writes captures/<out>/flags_<tag>.txt as `ADDR: xx xx ...` lines (16/line).
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_FLAG_TAG.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local sstate = require("probe.sstate")
local bp     = require("probe.bp")

local FIELD_BP=0x8001698C; local SCENE_NAME=0x8007050C; local GM=0x8007B83C
local FLAG_BASE=0x80085758; local FLAG_LEN=0x400
local SC_BLOCK=0x80084708          -- lead character record base

local START_SAVE = env.getenv("LEGAIA_SSTATE", "")
local OUT_DIR    = env.getenv("LEGAIA_OUT_DIR", "captures/storyflags")
local TAG        = env.getenv("LEGAIA_FLAG_TAG", "state")
local SETTLE0    = tonumber(env.getenv("LEGAIA_SETTLE0", "60")) or 60
local START_DELAY= tonumber(env.getenv("LEGAIA_BOOT_DELAY", "2")) or 2

os.execute(string.format("mkdir -p %q", OUT_DIR))
local OUTF = OUT_DIR .. "/flags_" .. TAG .. ".txt"
local F = io.open(OUTF, "w")
local function out(s) if F then F:write(s.."\n") end PCSX.log("[flags] "..s) end
local function ru8(a) return mem.in_ram(a) and mem.read_u8(a) or 0 end
local function read_scene()
    local s={}; for i=0,7 do local b=ru8(SCENE_NAME+i); if b<0x20 or b>=0x7f then break end; s[#s+1]=string.char(b) end
    return table.concat(s)
end
local function dump_region(base, len)
    for off=0,len-1,16 do
        local parts={}
        for i=0,15 do parts[#parts+1]=string.format("%02X", ru8(base+off+i)) end
        out(string.format("%08X: %s", base+off, table.concat(parts," ")))
    end
end

local vsync, loaded = 0, false
-- keep the handle: a GC'd listener object deletes the C++ listener
-- (silently unregisters; GC mid-dispatch can segfault the emulator)
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync=vsync+1
    if not loaded and START_SAVE~="" and vsync>=START_DELAY then
        loaded=true
        PCSX.log(sstate.load(START_SAVE) and ("[flags] resumed "..START_SAVE) or ("[flags] FAILED "..START_SAVE))
    end
end)

local frame, done = 0, false
bp.arm(FIELD_BP, "Exec", 4, "field_tick", function()
    frame=frame+1
    if frame<=SETTLE0 or done then return end
    done=true
    out(string.format("# tag=%s scene=%q mode=0x%02X", TAG, read_scene(), ru8(GM)))
    out("# story-flag bank 0x80085758:")
    dump_region(FLAG_BASE, FLAG_LEN)
    out("# lead char record 0x80084708 (+0x00..+0x40):")
    dump_region(SC_BLOCK, 0x40)
    if F then F:close() end
    PCSX.log("[flags] wrote "..OUTF)
    PCSX.quit(0)
end)

PCSX.log("[flags] dump_storyflags armed tag="..TAG)
