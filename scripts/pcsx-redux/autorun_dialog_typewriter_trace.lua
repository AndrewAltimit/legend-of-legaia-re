-- autorun_dialog_typewriter_trace.lua
--
-- Per-vsync trace of the field dialog pager's typewriter state
-- (`FUN_801D84D0`, field overlay PROT 0897; docs/formats/dialog-font.md
-- #typewriter-pacing). Opens one NPC's conversation the way
-- autorun_talk_to_npc.lua does (position poke, face, CROSS), then writes
-- one CSV row per vsync:
--
--   v, dt, state, counter, acc, hold, row, rows_on_page, rows, skip,
--   ptr0..ptr3 (row pointers, as offsets from the NPC's +0x90 record),
--   scroll, press
--
-- dt       = DAT_1F800393 (vsyncs per game tick; the pager runs once a tick)
-- state    = _DAT_801F2734 (pager state; 0x0B typing, 0x19 page end)
-- counter  = _DAT_801F2748 (reveal counter - the row's drawn glyph cap)
-- acc      = _DAT_801F2758 (speed accumulator)
-- hold     = _DAT_801F275C (short-row hold)
-- row      = _DAT_801F3530 (row being typed), rows_on_page = _DAT_801F3534,
-- rows     = _DAT_801F2740, skip = _DAT_801F2750 (0x24 / 0x25 skip latch)
-- scroll   = _DAT_801F2738 (row scroll, 1/16 px; the draw adds scroll >> 4)
-- press    = 1 on a vsync the probe holds CROSS, else 0
--
-- Counts and addresses only - no text bytes leave the emulator. The row
-- bytes are the disc's (the NPC record in the scene MAN), so the engine side
-- of the comparison reads them from the disc.
--
-- After a page ends (state 0x19) the probe waits LEGAIA_PAGE_WAIT vsyncs and
-- taps CROSS to turn it, so every page of the conversation is traced.
--
-- LEGAIA_SKIP_PAGES ("page:delay,...") also taps CROSS `delay` vsyncs after
-- that page's first typing vsync (state 0x0B), to trace the confirm-completes-
-- the-page arms (skip latch 0x25, state 0x0D). Pages count from 0 and advance
-- each time the pager leaves state 0x19.
--
-- Recompiler-safe (vsync-driven, no breakpoints); run with --fast.
-- Env: LEGAIA_SSTATE, LEGAIA_NPC_P90 (hex), LEGAIA_POKE_POS ("x,z"),
--      LEGAIA_TRACE_VSYNCS (default 900), LEGAIA_PAGE_WAIT (default 40),
--      LEGAIA_SKIP_PAGES (default none), LEGAIA_OUT_DIR.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env    = require("probe.env")
local mem    = require("probe.mem")
local pad    = require("probe.pad")
local sstate = require("probe.sstate")

local PLAYER_PTR = 0x8007C364
local LIST_HEADS = { 0x8007C34C, 0x8007C350, 0x8007C354, 0x8007C358, 0x8007C35C, 0x8007C360 }
local OUT_DIR = env.getenv("LEGAIA_OUT_DIR", "captures/dialog_typewriter_trace")
local START   = env.getenv("LEGAIA_SSTATE", "")
local P90     = tonumber(env.getenv("LEGAIA_NPC_P90", "0")) or 0
local TRACE_V = tonumber(env.getenv("LEGAIA_TRACE_VSYNCS", "900")) or 900
local WAIT    = tonumber(env.getenv("LEGAIA_PAGE_WAIT", "40")) or 40
local SKIPS = {}
for pg, dl in string.gmatch(env.getenv("LEGAIA_SKIP_PAGES", ""), "(%d+):(%d+)") do
    SKIPS[tonumber(pg)] = tonumber(dl)
end
local POKE = nil
do
    local px, pz = string.match(env.getenv("LEGAIA_POKE_POS", ""), "(%-?%d+),(%-?%d+)")
    if px then POKE = { tonumber(px), tonumber(pz) } end
end

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/trace.log", "w")
local function log(s) PCSX.log("[tw] " .. s); if LOG then LOG:write(s .. "\n"); LOG:flush() end end
local function s16(a)
    local v = mem.read_u16(a) or 0
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end
local function s32(a)
    local v = mem.read_u32(a) or 0
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end
local function pos(a) return s16(a + 0x14), s16(a + 0x18) end
local function find_npc()
    for _, h in ipairs(LIST_HEADS) do
        local a, n = mem.read_u32(h), 0
        while a and a ~= 0 and mem.in_ram(a) and n < 400 do
            if mem.read_u32(a + 0x90) == P90 then return a end
            a = mem.read_u32(a); n = n + 1
        end
    end
    return nil
end
local function dir_to(dx, dz)
    if math.abs(dx) > math.abs(dz) then
        return dx > 0 and pad.BTN.RIGHT or pad.BTN.LEFT
    end
    return dz > 0 and pad.BTN.UP or pad.BTN.DOWN
end

local CSV = nil
local v, loaded, npc, phase, press_v, t0 = 0, false, nil, "SETUP", nil, nil
local held = {}
local function hold(b) if not held[b] then pad.force(b); held[b] = true end end
local function release_all() for b in pairs(held) do pad.release(b) end; held = {} end
local page_end_v, release_v = nil, nil
local page, page_type_v, skip_done, prev_st = 0, nil, {}, nil

local function trace_row(t)
    local function ptr(i)
        local p = mem.read_u32(0x801F3540 + i * 4) or 0
        if p == 0 then return "" end
        return string.format("%d", p - P90)
    end
    local pressed = 0
    if held[pad.BTN.CROSS] then pressed = 1 end
    CSV:write(string.format("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%s,%s,%s,%s,%d,%d\n",
        t, mem.read_scratch_u8(0x1F800393),
        s32(0x801F2734), s32(0x801F2748), s32(0x801F2758), s32(0x801F275C),
        s32(0x801F3530), s32(0x801F3534), s32(0x801F2740), s32(0x801F2750),
        ptr(0), ptr(1), ptr(2), ptr(3), s32(0x801F2738), pressed))
end

local function on_vsync()
    v = v + 1
    if not loaded then
        if v >= 2 then
            loaded = true
            log((sstate.load(START) and "resumed " or "FAILED to load ") .. START)
        end
        return
    end
    if v < 20 then return end
    local pl = mem.read_u32(PLAYER_PTR)
    if not pl or not mem.in_ram(pl) then return end
    if POKE then
        mem.write_u16(pl + 0x14, POKE[1] % 0x10000); mem.write_u16(pl + 0x18, POKE[2] % 0x10000)
        log(string.format("vsync %d: poked player to (%d,%d)", v, POKE[1], POKE[2]))
        POKE = nil
        return
    end
    npc = npc or find_npc()
    if not npc then
        if v > 300 then log("npc never appeared; quitting"); PCSX.quit(1) end
        return
    end
    if phase == "SETUP" then
        -- Face, then CROSS; retry every 30 vsyncs until the pager holds a
        -- row pointer. The trace clock starts at the first CROSS press.
        local px, pz = pos(pl)
        local nx, nz = pos(npc)
        press_v = press_v or v
        local t = (v - press_v) % 30
        if t == 0 then release_all(); hold(dir_to(nx - px, nz - pz))
        elseif t == 4 then release_all()
        elseif t == 10 then hold(pad.BTN.CROSS); t0 = t0 or v
        elseif t == 14 then release_all() end
        if t0 then
            if not CSV then
                CSV = io.open(OUT_DIR .. "/trace.csv", "w")
                CSV:write("t,dt,state,counter,acc,hold,row,rows_on_page,rows,skip,ptr0,ptr1,ptr2,ptr3,scroll,press\n")
                log(string.format("vsync %d: first CROSS; player (%d,%d) npc (%d,%d)", v, px, pz, nx, nz))
            end
            trace_row(v - t0)
            if (mem.read_u32(0x801F3540) or 0) ~= 0 and t >= 14 then
                release_all()
                phase = "TRACE"
                log(string.format("vsync %d: pager state %d", v, s32(0x801F2734)))
            end
        end
        if v - press_v > 600 then log("no conversation after 20 tries"); PCSX.quit(1) end
        return
    end
    -- TRACE: per-vsync row; turn each page WAIT vsyncs after it ends.
    local t = v - t0
    trace_row(t)
    local st = s32(0x801F2734)
    if release_v and v >= release_v then release_all(); release_v = nil end
    if prev_st == 0x19 and st ~= 0x19 then page = page + 1; page_type_v = nil end
    prev_st = st
    if st == 0x0B and not page_type_v then page_type_v = v end
    local dl = SKIPS[page]
    if dl and page_type_v and not skip_done[page] and not release_v
        and v - page_type_v == dl then
        hold(pad.BTN.CROSS); release_v = v + 4; skip_done[page] = true
        log(string.format("vsync %d: page %d skip tap (state %d)", v, page, st))
    end
    if st == 0x19 and not release_v then
        page_end_v = page_end_v or v
        if v - page_end_v == WAIT then
            hold(pad.BTN.CROSS); release_v = v + 4; page_end_v = nil
        end
    elseif st ~= 0x19 then
        page_end_v = nil
    end
    if t >= TRACE_V then
        CSV:close(); log("trace done"); PCSX.quit(0)
    end
end

PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
log(string.format("armed: npc p90=%08X trace=%d wait=%d", P90, TRACE_V, WAIT))
