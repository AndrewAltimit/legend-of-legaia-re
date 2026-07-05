-- autorun_text_census.lua
--
-- Diagnostic: which text path renders the "It was the Seru." caption during
-- opdeene? Neither the balloon spawner (FUN_8003C764) nor the text-actor
-- register (FUN_8003541C) fired across the whole opdeene leg, yet the caption
-- displays at ~830 field ticks into opdeene. This probe arms EVERY candidate
-- text function with a global hit counter, logs the first hit of each (with the
-- relevant pointer arg + bytes), and dumps every firing function's pointer arg
-- during the caption window [rel 810..870]. Whichever fires there is the path;
-- its pointer arg's VA pins the caption source.
--
-- Cold-boot title driver identical to autorun_crawl1_capture.lua.
--
-- Run:
--   LEGAIA_OUT_DIR=captures/text_census DISPLAY=:0 timeout --kill-after=15s 900 \
--     ~/Tools/pcsx-redux/pcsx-redux -interpreter -debugger -fastboot \
--     -bios ~/.mednafen/firmware/SCPH1001.BIN -iso "$LEGAIA_DISC_BIN" -run -stdout \
--     -dofile scripts/pcsx-redux/autorun_text_census.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env = require("probe.env")
local mem = require("probe.mem")
local pad = require("probe.pad")
local bp  = require("probe.bp")

local GM         = 0x8007B83C
local SCENE_NAME = 0x8007050C
local TITLE_BP   = 0x801DD35C
local FIELD_BP   = 0x8001698C

-- Candidate text functions: {addr, label, which arg reg carries the text ptr}
local FUNCS = {
    { 0x8003541C, "REGISTER",  "a2" }, -- text-actor register (record_string)
    { 0x8003C764, "SPAWNER",   "a0" }, -- balloon spawner (text ptr)
    { 0x80037174, "CRAWL",     "a0" }, -- crawl roller
    { 0x80036888, "MESREND",   "a0" }, -- MES text renderer (buf)
    { 0x8003CC98, "LINE",      "a0" }, -- single-line render+measure
    { 0x8003C1F8, "GLYPH",     "a0" }, -- per-glyph sprite emitter (cell_idx)
    { 0x8003C764, "SPAWN2",    "a0" },
}

local OUT_DIR       = env.getenv("LEGAIA_OUT_DIR", "captures/text_census")
local OPDEENE_TICKS = tonumber(env.getenv("LEGAIA_OPDEENE_TICKS", "3200")) or 3200
local WIN_LO        = tonumber(env.getenv("LEGAIA_WIN_LO", "810")) or 810
local WIN_HI        = tonumber(env.getenv("LEGAIA_WIN_HI", "870")) or 870
local TITLE_MAX     = tonumber(env.getenv("LEGAIA_TITLE_MAX", "40000")) or 40000
local MASH_EVERY    = tonumber(env.getenv("LEGAIA_MASH_EVERY", "20")) or 20

os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/census.log", "w")
local function log(s)
    PCSX.log("[cen] " .. s)
    if LOG then LOG:write(s .. "\n"); LOG:flush() end
end

local function tou32(v) v = v or 0; if v < 0 then return v + 0x100000000 end; return v end
local function read_mode() return mem.in_ram(GM) and mem.read_u8(GM) or nil end
local function read_scene()
    if not mem.in_ram(SCENE_NAME) then return "" end
    local s = {}
    for i = 0, 7 do
        local b = mem.read_u8(SCENE_NAME + i) or 0
        if b < 0x20 or b >= 0x7f then break end
        s[#s + 1] = string.char(b)
    end
    return table.concat(s)
end
local function dump(addr, len)
    if not mem.in_ram(addr) then return "(not in RAM)" end
    local hex, asc = {}, {}
    for i = 0, len - 1 do
        local b = mem.read_u8(addr + i) or 0
        hex[#hex + 1] = string.format("%02X", b)
        asc[#asc + 1] = (b >= 0x20 and b < 0x7f) and string.char(b) or "."
    end
    return table.concat(hex, " ") .. "  |" .. table.concat(asc) .. "|"
end

local PHASE = "TITLE"
local g_title_tick, g_tick, g_pulse, g_release_at = 0, 0, 0, 0
local g_held = {}
local cur_scene, opdeene_enter_tick = "", 0
local g_quit_at = nil
local g_counts = {}   -- label -> total hit count
local g_first = {}    -- label -> logged first hit yet?
local g_win_logged = {} -- label -> logged in window count

local function hold(b) pad.force(b); g_held[#g_held + 1] = b end
local function release_all()
    for _, b in ipairs(g_held) do pad.release(b) end
    g_held = {}
end
local function finish(code, why)
    if PHASE == "DONE" then return end
    PHASE = "DONE"; release_all()
    local parts = {}
    for _, f in ipairs(FUNCS) do
        parts[#parts + 1] = string.format("%s=%d", f[2], g_counts[f[2]] or 0)
    end
    log("done: " .. why .. "  counts[" .. table.concat(parts, " ") .. "]")
    if LOG then LOG:close() end
    g_quit_at = { code = code, at = g_tick + 2, title_at = g_title_tick + 2 }
end

local PATTERN = { { pad.BTN.START }, { pad.BTN.UP }, { pad.BTN.CROSS } }
local function opening_reached() return read_mode() == 3 and read_scene() == "opdeene" end
local function enter_capture(from)
    release_all(); PHASE = "CAPTURE"
    cur_scene = read_scene(); opdeene_enter_tick = g_tick
    log(string.format("CAPTURE start (%s): scene=%q title=%d field=%d",
        from, cur_scene, g_title_tick, g_tick))
end

-- Decode a null/terminator-bounded ASCII string at addr (MES buffers are plain
-- ASCII here; stop at 0x00 or a control byte < 0x09).
local function decode_ascii(addr, maxlen)
    local out = {}
    for i = 0, maxlen - 1 do
        local b = mem.read_u8(addr + i)
        if b == nil or b == 0 then break end
        if b < 0x09 then break end
        if b >= 0x20 and b < 0x7f then out[#out + 1] = string.char(b)
        else out[#out + 1] = string.format("[%02X]", b) end
    end
    return table.concat(out)
end

local g_seen_ptr = {}
local function make_handler(label, argreg)
    return function()
        g_counts[label] = (g_counts[label] or 0) + 1
        if PHASE ~= "CAPTURE" then return end
        local r = PCSX.getRegisters()
        local n = r.GPR and r.GPR.n
        if not n then return end
        local ptr = tou32(n[argreg])
        local rel = g_tick - opdeene_enter_tick
        -- Enumerate every DISTINCT text-buffer pointer per function, decoded as
        -- ASCII. Dedup key = label:ptr:firstword so a reused buffer with new
        -- content still logs. This yields the full opdeene text corpus with each
        -- string's source VA (the caption source we are hunting).
        if label == "MESREND" or label == "CRAWL" or label == "REGISTER"
            or label == "SPAWNER" or label == "LINE" then
            if mem.in_ram(ptr) then
                local fw = mem.read_u32(ptr) or 0
                local key = string.format("%s:%08X:%08X", label, ptr, fw)
                if not g_seen_ptr[key] then
                    g_seen_ptr[key] = true
                    log(string.format("STR %s rel=%d %s=0x%08X ra=0x%08X  %q",
                        label, rel, argreg, ptr, tou32(n.ra), decode_ascii(ptr, 48)))
                end
            end
        end
    end
end

local function title_tick()
    g_title_tick = g_title_tick + 1
    if PHASE == "DONE" then
        if g_quit_at and g_title_tick >= g_quit_at.title_at then PCSX.quit(g_quit_at.code) end
        return
    end
    if PHASE ~= "TITLE" then return end
    local scene = read_scene()
    if opening_reached() then enter_capture("title"); return end
    if scene ~= "opdeene" and scene ~= "" and read_mode() == 3 then
        log("WRONG_PATH: scene " .. scene); finish(1, "wrong path"); return
    end
    if g_title_tick >= TITLE_MAX then finish(1, "title timeout"); return end
    if g_release_at > 0 and g_title_tick >= g_release_at then
        release_all(); g_release_at = 0
    elseif g_release_at == 0 and (g_title_tick % MASH_EVERY) == 0 then
        g_pulse = g_pulse + 1
        for _, b in ipairs(PATTERN[(g_pulse % #PATTERN) + 1]) do hold(b) end
        g_release_at = g_title_tick + 5
    end
end

local function field_tick()
    g_tick = g_tick + 1
    if PHASE == "DONE" then
        if g_quit_at and g_tick >= g_quit_at.at then PCSX.quit(g_quit_at.code) end
        return
    end
    if PHASE == "TITLE" then
        if opening_reached() then enter_capture("field") end
        return
    end
    local scene = read_scene()
    if scene ~= cur_scene and scene ~= "" then
        finish(0, string.format("opdeene ended -> %q", scene)); return
    end
    if (g_tick % 300) == 0 then
        log(string.format("...tick %d (rel %d)", g_tick, g_tick - opdeene_enter_tick))
    end
    if g_tick - opdeene_enter_tick >= OPDEENE_TICKS then finish(0, "budget") end
end

pcall(function() bp.arm(TITLE_BP, "Exec", 4, "title_tick", title_tick) end)
pcall(function() bp.arm(FIELD_BP, "Exec", 4, "field_tick", field_tick) end)
local seen_addr = {}
for _, f in ipairs(FUNCS) do
    if not seen_addr[f[1]] then
        seen_addr[f[1]] = true
        local label, argreg = f[2], f[3]
        pcall(function() bp.arm(f[1], "Exec", 4, "tx_" .. label, make_handler(label, argreg)) end)
    end
end
log("text census armed")

PCSX.Events.createEventListener("GPU::Vsync", function()
    if PHASE == "DONE" and g_quit_at then
        g_quit_at.vs_seen = (g_quit_at.vs_seen or 0) + 1
        if g_quit_at.vs_seen > 10 then PCSX.quit(g_quit_at.code) end
    end
end)
