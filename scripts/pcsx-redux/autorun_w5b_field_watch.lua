-- autorun_w5b_field_watch.lua
--
-- One configurable field-mode watch: load a state, drive a pad schedule and
-- optional position pokes, and record every write to a set of globals with
-- the value that LANDS (the store at PC is decoded and its source register
-- read, because the debug hook runs before the store), every hit on a set of
-- exec addresses with its argument registers, and a per-vsync sample of a
-- set of words. Built to confirm disassembly-only claims against retail
-- without writing a new probe per claim: the clip-base writers, the kind-0
-- warp timer pair, a talk-end PC, an audio latch.
--
-- Frame numbers in every output are vsyncs since the state load (`f`),
-- so a CSV row can be lined up with a screenshot or a checkpoint.
--
-- Env (all optional except LEGAIA_SSTATE):
--   LEGAIA_SSTATE    save state to load
--   LEGAIA_WATCH     "addr:width:label,..." write watches (width 1/2/4)
--   LEGAIA_EXEC      "addr:label,..." exec breakpoints (logs a0..a3, ra, s8)
--   LEGAIA_READ      "addr:width:label,..." read watches, logged into the exec
--                    CSV (label prefixed `r_`) with the reading PC
--   LEGAIA_EXEC_MAX  hits per exec / read label before it stops logging
--                    (default 400)
--   LEGAIA_SAMPLE    "addr:width:label,..." per-vsync samples; an address
--                    of the form `P+0xNN` is an offset into the player
--                    actor (`*(0x8007C364)`), and `*0xADDR+0xNN` an offset
--                    through the pointer word at ADDR
--   LEGAIA_PRESS     "frame:BTN[+BTN]:dur,..." pad holds, frames after load
--   LEGAIA_POKE      "frame:x:z,..." write player +0x14/+0x18 on that frame
--   LEGAIA_MEMPOKE   "frame:addr:width:value,..." one-shot memory writes
--   LEGAIA_POKE_HOLD vsyncs a poke is re-applied (default 1)
--   LEGAIA_SHOTS     "frame,..." framebuffer screenshots (<out>/shot_<f>.raw)
--   LEGAIA_CKPTS     "frame,..." raw save states (<out>/ckpt_<f>.rawsstate)
--   LEGAIA_DUMPS     "frame:addr:len,..." hex dumps into the log; `P+0xNN`
--                    as above, `L` = the first live actor whose handler
--                    +0x0C equals LEGAIA_DUMP_HANDLER (the light actor)
--   LEGAIA_ACTORS    "frame,..." log every actor on the six field list heads
--   LEGAIA_TRIGGERS  1 = log the scene's kind-0 / kind-1 trigger tables
--                    (field buffer *(0x1F8003EC) + 0x10000) once after load
--   LEGAIA_FRAMES    vsyncs after load before quitting (default 900)
--   LEGAIA_OUT_DIR   output directory
--
-- Output: w5b_writes.csv, w5b_exec.csv, w5b_samples.csv, w5b.log.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local bp    = require("probe.bp")
local pad   = require("probe.pad")
local bit   = require("bit")

local GAME_MODE  = 0x8007B83C
local SCENE_NAME = 0x8007050C
local PLAYER_PTR = 0x8007C364
local FIELD_BUF  = 0x1F8003EC

local SSTATE     = probe.getenv("LEGAIA_SSTATE", "")
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 900)
local EXEC_MAX   = probe.getenv_num("LEGAIA_EXEC_MAX", 400)
local POKE_HOLD  = probe.getenv_num("LEGAIA_POKE_HOLD", 1)
local WANT_TRIG  = probe.getenv("LEGAIA_TRIGGERS", "") == "1"
local DUMP_HANDLER = tonumber(probe.getenv("LEGAIA_DUMP_HANDLER", "0")) or 0
local OUT_DIR    = probe.getenv("LEGAIA_OUT_DIR", "captures/w5b_field_watch")

local function n32(v) v = tonumber(v) or 0; return bit.band(v, 0xFFFFFFFF) % 4294967296 end
local function hex32(v) return string.format("0x%08X", n32(v)) end
local function u8(a) return mem.read_u8(a) or 0 end
local function u16(a) return mem.read_u16(a) or 0 end
local function u32(a) return n32(mem.read_u32(a) or 0) end
local function s16(v) v = v % 0x10000; if v >= 0x8000 then v = v - 0x10000 end; return v end
local function s32(v) v = n32(v); if v >= 0x80000000 then v = v - 4294967296 end; return v end

local LOGF
local function log(s)
    PCSX.log("[w5b] " .. s)
    if LOGF then LOGF:write(s .. "\n"); LOGF:flush() end
end

local function scene_name()
    local s = {}
    for i = 0, 7 do
        local b = u8(SCENE_NAME + i)
        if b < 0x20 or b >= 0x7F then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end

local function player_ptr()
    local p = u32(PLAYER_PTR)
    if p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end

local function player_xz()
    local p = player_ptr()
    if p == nil then return 0, 0 end
    return s16(u16(p + 0x14)), s16(u16(p + 0x18))
end

-- "P+0x10" -> player-relative; "*0x801C6EA4+0x62" -> through a pointer
-- word; plain hex -> absolute.
local function resolve(spec)
    local pw, poff = string.match(spec, "^%*(0x%x+)%+(0x%x+)$")
    if pw then
        local p = u32(tonumber(pw))
        if p < 0x80000000 or p >= 0x80200000 then return nil end
        return p + tonumber(poff)
    end
    local off = string.match(spec, "^P%+(0x%x+)$")
    if off then
        local p = player_ptr()
        if p == nil then return nil end
        return p + tonumber(off)
    end
    return tonumber(spec)
end

local function split(env, sep_items)
    local out = {}
    for tok in string.gmatch(probe.getenv(env, ""), "[^,%s]+") do
        local parts = {}
        for p in string.gmatch(tok, "[^:]+") do parts[#parts + 1] = p end
        out[#out + 1] = parts
    end
    return out
end

local WATCHES = {}
for _, p in ipairs(split("LEGAIA_WATCH")) do
    WATCHES[#WATCHES + 1] = { addr = tonumber(p[1]), width = tonumber(p[2]) or 4, label = p[3] or p[1] }
end
local EXECS = {}
for _, p in ipairs(split("LEGAIA_EXEC")) do
    EXECS[#EXECS + 1] = { addr = tonumber(p[1]), label = p[2] or p[1], hits = 0 }
end
for _, p in ipairs(split("LEGAIA_READ")) do
    EXECS[#EXECS + 1] = { addr = tonumber(p[1]), width = tonumber(p[2]) or 4, label = "r_" .. (p[3] or p[1]), hits = 0, kind = "Read" }
end
local SAMPLES = {}
for _, p in ipairs(split("LEGAIA_SAMPLE")) do
    SAMPLES[#SAMPLES + 1] = { spec = p[1], width = tonumber(p[2]) or 4, label = p[3] or p[1] }
end
local PRESSES = {}
for _, p in ipairs(split("LEGAIA_PRESS")) do
    local btns = {}
    for name in string.gmatch(p[2], "[^+]+") do
        local b = pad.BTN[string.upper(name)]
        if b == nil then error("LEGAIA_PRESS: unknown button " .. name) end
        btns[#btns + 1] = b
    end
    PRESSES[#PRESSES + 1] = { at = tonumber(p[1]), btns = btns, dur = tonumber(p[3]), name = p[2] }
end
-- "frame:addr:width:value" raw memory pokes (e.g. the encounter step
-- counter), applied once on that frame.
local MEMPOKES = {}
for _, p in ipairs(split("LEGAIA_MEMPOKE")) do
    MEMPOKES[#MEMPOKES + 1] = { at = tonumber(p[1]), addr = tonumber(p[2]), width = tonumber(p[3]), value = tonumber(p[4]) }
end
local POKES = {}
for _, p in ipairs(split("LEGAIA_POKE")) do
    POKES[#POKES + 1] = { at = tonumber(p[1]), x = tonumber(p[2]), z = tonumber(p[3]) }
end
local SHOTS, CKPTS = {}, {}
for tok in string.gmatch(probe.getenv("LEGAIA_SHOTS", ""), "[^,%s]+") do SHOTS[tonumber(tok)] = true end
for tok in string.gmatch(probe.getenv("LEGAIA_CKPTS", ""), "[^,%s]+") do CKPTS[tonumber(tok)] = true end
local DUMPS = split("LEGAIA_DUMPS")
local ACTORS_AT = {}
for tok in string.gmatch(probe.getenv("LEGAIA_ACTORS", ""), "[^,%s]+") do ACTORS_AT[tonumber(tok)] = true end

os.execute(string.format("mkdir -p %q", OUT_DIR))
LOGF = io.open(OUT_DIR .. "/w5b.log", "w")
local WCSV = probe.csv_open(OUT_DIR .. "/w5b_writes.csv", "f,label,addr,pc,ra,old,new,scene,mode,px,pz")
local ECSV = probe.csv_open(OUT_DIR .. "/w5b_exec.csv", "f,label,pc,ra,a0,a1,a2,a3,s8,v0,s5,s6,s1,s7,scene,mode,px,pz")
local shdr = { "f", "scene", "mode", "px", "pz", "held" }
for _, s in ipairs(SAMPLES) do shdr[#shdr + 1] = s.label end
local SCSV = probe.csv_open(OUT_DIR .. "/w5b_samples.csv", table.concat(shdr, ","))

local f, vsync = -1, 0
local loaded, armed, done = false, false, false
local held_names = {}

local function read_width(a, w)
    if w == 1 then return u8(a) elseif w == 2 then return u16(a) end
    return u32(a)
end

local function stored_value()
    local r    = PCSX.getRegisters()
    local pc   = n32(r.pc)
    local insn = u32(pc)
    local op = bit.rshift(insn, 26)
    local rt = bit.band(bit.rshift(insn, 16), 0x1F)
    local v  = n32(r.GPR.r[rt])
    if op == 0x28 then return bit.band(v, 0xFF), pc, r
    elseif op == 0x29 then return bit.band(v, 0xFFFF), pc, r
    elseif op == 0x2B then return v, pc, r end
    return v, pc, r
end

local function screenshot(tag)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or not ss then log("screenshot failed at f=" .. f); return end
    local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
    local h = io.open(OUT_DIR .. "/shot_" .. tag .. ".raw", "wb"); h:write(tostring(ss.data)); h:close()
    local m = io.open(OUT_DIR .. "/shot_" .. tag .. ".raw.meta", "w")
    m:write(string.format("width=%d\nheight=%d\nbpp=%d\n", tonumber(ss.width), tonumber(ss.height), bpp)); m:close()
    log(string.format("shot f=%d %dx%d bpp=%d", f, tonumber(ss.width), tonumber(ss.height), bpp))
end

local function checkpoint(tag)
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(OUT_DIR .. "/ckpt_" .. tag .. ".rawsstate", "CREATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    log("checkpoint f=" .. f .. " " .. tostring(ok) .. " " .. tostring(err))
end

-- The first live actor whose handler word +0x0C matches: the SCUS actor
-- list head is walked through the +0x00 next link.
local ACTOR_HEAD = tonumber(probe.getenv("LEGAIA_ACTOR_HEAD", "0x8007C360")) or 0x8007C360
local function find_actor(handler)
    local p = u32(ACTOR_HEAD)
    for _ = 1, 512 do
        if p < 0x80000000 or p >= 0x80200000 then return nil end
        if u32(p + 0x0C) == handler then return p end
        p = u32(p)
    end
    return nil
end

local function dump(parts)
    local a
    if parts[2] == "L" then a = find_actor(DUMP_HANDLER) else a = resolve(parts[2]) end
    local len = tonumber(parts[3]) or 0x40
    if a == nil then log("dump " .. parts[2] .. ": unresolved at f=" .. f); return end
    local out = {}
    for i = 0, len - 1 do out[#out + 1] = string.format("%02X", u8(a + i)) end
    log(string.format("dump f=%d %s @%s len=0x%X: %s", f, parts[2], hex32(a), len, table.concat(out, " ")))
end

local function log_triggers()
    local base = n32(mem.read_scratch_u32(FIELD_BUF))
    log(string.format("field buffer *(0x1F8003EC) = %s", hex32(base)))
    if base < 0x80000000 or base >= 0x80200000 then return end
    for _, blk in ipairs({ 0x10000, 0x12000 }) do
        local b = base + blk
        for kind = 0, 2 do
            local off = s16(u16(b + 4 * kind + 2))
            local cnt = s16(u16(b + 4 * kind + 4))
            local rows = {}
            if off >= 0 and cnt > 0 and cnt < 512 then
                for i = 0, cnt - 1 do
                    local r = b + off + i * 4
                    rows[#rows + 1] = string.format("(%d,%d:%d,%d)", u8(r), u8(r + 1), u8(r + 2), u8(r + 3))
                end
            end
            log(string.format("block +0x%X kind %d off=%d count=%d %s", blk, kind, off, cnt, table.concat(rows, " ")))
        end
    end
end

-- Every actor on the six field list heads: handler, position, script base.
local LIST_HEADS = { 0x8007C34C, 0x8007C350, 0x8007C354, 0x8007C358, 0x8007C35C, 0x8007C360 }
local function log_actors()
    for _, h in ipairs(LIST_HEADS) do
        local p, n = u32(h), 0
        while p >= 0x80000000 and p < 0x80200000 and n < 300 do
            log(string.format("actor head=%s @%s h=%s pos=(%d,%d,%d) +90=%s +94=%s +10=%s +50=%d",
                hex32(h), hex32(p), hex32(u32(p + 0x0C)), s16(u16(p + 0x14)), s16(u16(p + 0x16)),
                s16(u16(p + 0x18)), hex32(u32(p + 0x90)), hex32(u32(p + 0x94)), hex32(u32(p + 0x10)),
                s16(u16(p + 0x50))))
            p = u32(p); n = n + 1
        end
    end
end

local function ctx_cols()
    local px, pz = player_xz()
    return scene_name(), u8(GAME_MODE), px, pz
end

local function arm_all()
    for _, w in ipairs(WATCHES) do
        bp.arm(w.addr, "Write", w.width, "w_" .. w.label, function()
            local v, pc, r = stored_value()
            local sc, md, px, pz = ctx_cols()
            WCSV:row("%d,%s,%s,%s,%s,%d,%d,%s,0x%02X,%d,%d", f, w.label, hex32(w.addr), hex32(pc),
                hex32(r.GPR.n.ra), s32(read_width(w.addr, w.width)), s32(v), sc, md, px, pz)
            WCSV.fh:flush()
        end)
    end
    for _, e in ipairs(EXECS) do
        bp.arm(e.addr, e.kind or "Exec", e.width or 4, "x_" .. e.label, function()
            e.hits = e.hits + 1
            if e.hits > EXEC_MAX then return end
            local r = PCSX.getRegisters()
            local sc, md, px, pz = ctx_cols()
            ECSV:row("%d,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,0x%02X,%d,%d", f, e.label, hex32(r.pc), hex32(r.GPR.n.ra),
                hex32(r.GPR.n.a0), hex32(r.GPR.n.a1), hex32(r.GPR.n.a2), hex32(r.GPR.n.a3),
                hex32(r.GPR.n.s8), hex32(r.GPR.n.v0), hex32(r.GPR.n.s5), hex32(r.GPR.n.s6),
                hex32(r.GPR.n.s1), hex32(r.GPR.n.s7), sc, md, px, pz)
            ECSV.fh:flush()
        end)
    end
    armed = true
    log(string.format("armed f=%d: %d watch, %d exec, %d sample, %d press, %d poke",
        f, #WATCHES, #EXECS, #SAMPLES, #PRESSES, #POKES))
end

local function finish(why)
    if done then return end
    done = true
    for _, e in ipairs(EXECS) do log(string.format("exec %s hits=%d", e.label, e.hits)) end
    log(string.format("%s at f=%d scene=%s mode=0x%02X", why, f, scene_name(), u8(GAME_MODE)))
    pcall(function() bp.disarm() end)
    WCSV:close(); ECSV:close(); SCSV:close()
    if LOGF then LOGF:close() end
    PCSX.quit(0)
end

local last_key = nil
local function on_vsync()
    if done then return end
    vsync = vsync + 1
    if not loaded then
        if vsync >= BOOT_DELAY then
            if SSTATE ~= "" and not probe.load_save_state(SSTATE) then
                log("FATAL: could not load " .. SSTATE); finish("load failed"); return
            end
            loaded = true; f = 0
            log(string.format("state loaded; scene=%s mode=0x%02X", scene_name(), u8(GAME_MODE)))
        end
        return
    end
    f = f + 1
    if not armed then
        arm_all()
        if WANT_TRIG then log_triggers() end
    end

    for _, p in ipairs(PRESSES) do
        if f == p.at then
            for _, b in ipairs(p.btns) do pad.force(b) end
            held_names[p] = p.name
            log(string.format("press %s f=%d for %d", p.name, f, p.dur))
        elseif f == p.at + p.dur then
            for _, b in ipairs(p.btns) do pad.release(b) end
            held_names[p] = nil
        end
    end
    for _, p in ipairs(POKES) do
        if f >= p.at and f < p.at + POKE_HOLD then
            local pp = player_ptr()
            if pp then
                mem.write_u16(pp + 0x14, p.x % 0x10000)
                mem.write_u16(pp + 0x18, p.z % 0x10000)
                if f == p.at then log(string.format("poke f=%d -> (%d,%d)", f, p.x, p.z)) end
            end
        end
    end
    for _, m in ipairs(MEMPOKES) do
        if f == m.at then
            if m.width == 1 then mem.write_u8(m.addr, m.value)
            elseif m.width == 2 then mem.write_u16(m.addr, m.value)
            else mem.write_u32(m.addr, m.value) end
            log(string.format("mempoke f=%d %s <- %d", f, hex32(m.addr), m.value))
        end
    end
    for _, d in ipairs(DUMPS) do if tonumber(d[1]) == f then dump(d) end end
    if ACTORS_AT[f] then log_actors() end

    local sc, md, px, pz = ctx_cols()
    local hn = {}
    for _, n in pairs(held_names) do hn[#hn + 1] = n end
    local row = { tostring(f), sc, string.format("0x%02X", md), tostring(px), tostring(pz), table.concat(hn, "+") }
    for _, s in ipairs(SAMPLES) do
        local a = resolve(s.spec)
        row[#row + 1] = a and tostring(s32(read_width(a, s.width))) or ""
    end
    SCSV.fh:write(table.concat(row, ",") .. "\n")
    local key = sc .. "|" .. md
    if key ~= last_key then
        last_key = key
        log(string.format("f=%d scene=%s mode=0x%02X player=(%d,%d)", f, sc, md, px, pz))
    end
    if SHOTS[f] then screenshot(tostring(f)) end
    if CKPTS[f] then checkpoint(tostring(f)) end
    if f >= FRAMES then finish("frames done") end
end

log("=== autorun_w5b_field_watch === sstate=" .. (SSTATE == "" and "(none)" or SSTATE))
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
