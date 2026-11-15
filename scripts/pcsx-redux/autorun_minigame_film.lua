-- autorun_minigame_film.lua
--
-- The retail side of the minigame film: load a minigame state, hold a pad
-- schedule, and write a named set of the minigame's RAM words once per vsync.
-- Its engine side is LEGAIA_RC_MINIGAME_FILM on `legaia-engine retail-compare`;
-- scripts/ci/minigame-film.py names the words per minigame and pairs the two
-- sides game frame by game frame. Poll-only, safe under --fast.
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_OUT_DIR,
--      LEGAIA_PAD_SCRIPT   `A-B:BTN` spans in vsyncs from the load.
--      LEGAIA_FILM_WORDS   comma-separated `name=ADDR:KIND`, ADDR hex, KIND one
--                          of u8 / s8 / u16 / s16 / u32 / s32; an ADDR in
--                          0x1F800000..0x1F8003FF reads the scratchpad (u8 /
--                          u32 only). `name=*PTR+OFF:KIND` reads through the
--                          word at PTR (both hex), e.g. a sequencer slot's
--                          record off its pointer table.
--
-- Output: minigame_film.csv, one row a vsync: `vsync,<name>...`.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 600)
local SCRIPT = probe.getenv("LEGAIA_PAD_SCRIPT", "")
local WORDS = probe.getenv("LEGAIA_FILM_WORDS", "")

local words = {}
for item in string.gmatch(WORDS, "[^,]+") do
    local name, addr, kind = string.match(item, "^%s*([%w_]+)=0[xX](%x+):(%a%d+)%s*$")
    local pname, ptr, off, pkind = string.match(item, "^%s*([%w_]+)=%*0[xX](%x+)%+0[xX](%x+):(%a%d+)%s*$")
    if name then
        words[#words + 1] = { name = name, addr = tonumber(addr, 16), kind = kind }
    elseif pname then
        words[#words + 1] = { name = pname, ptr = tonumber(ptr, 16), off = tonumber(off, 16), kind = pkind }
    else
        PCSX.log("[minigame-film] ignoring word '" .. item .. "'")
    end
end

local spans = {}
for item in string.gmatch(SCRIPT, "[^,]+") do
    local a, b, name = string.match(item, "^%s*(%d+)-(%d+):(%a+)%s*$")
    local btn = name and probe.BTN[string.upper(name)]
    if a and btn then
        spans[#spans + 1] = { from = tonumber(a), to = tonumber(b), btn = btn }
    else
        PCSX.log("[minigame-film] ignoring pad item '" .. item .. "'")
    end
end

local function signed(v, bits)
    if v == nil then return 0 end
    local top = 2 ^ (bits - 1)
    if v >= top then v = v - 2 * top end
    return v
end

local function read(w)
    if w.ptr then
        local base = probe.read_u32(w.ptr)
        if base == nil or not probe.in_ram(base) then return -1 end
        return read({ addr = base + w.off, kind = w.kind })
    end
    local scratch = w.addr >= 0x1F800000 and w.addr < 0x1F800400
    if w.kind == "u8" or w.kind == "s8" then
        local v = scratch and probe.read_scratch_u8(w.addr) or probe.read_u8(w.addr)
        return w.kind == "s8" and signed(v, 8) or (v or 0)
    elseif w.kind == "u16" or w.kind == "s16" then
        local v = probe.read_u16(w.addr)
        return w.kind == "s16" and signed(v, 16) or (v or 0)
    else
        local v = scratch and probe.read_scratch_u32(w.addr) or probe.read_u32(w.addr)
        return w.kind == "s32" and signed(v, 32) or (v or 0)
    end
end

local held = {}
local fh = assert(io.open(probe.out_path("minigame_film.csv"), "w"))
local header = { "vsync" }
for _, w in ipairs(words) do header[#header + 1] = w.name end
fh:write(table.concat(header, ",") .. "\n")

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_minigame_film.lua",
            { sstate = SSTATE, frames = FRAMES, pad = SCRIPT, words = WORDS })
        return {}
    end,
    on_capture = function(_, el)
        local want = {}
        for _, s in ipairs(spans) do
            if el >= s.from and el < s.to then want[s.btn] = true end
        end
        for btn, _ in pairs(held) do
            if not want[btn] then probe.pad_release(btn); held[btn] = nil end
        end
        for btn, _ in pairs(want) do
            if not held[btn] then probe.pad_force(btn); held[btn] = true end
        end
        local row = { tostring(el) }
        for _, w in ipairs(words) do row[#row + 1] = string.format("%d", read(w)) end
        fh:write(table.concat(row, ",") .. "\n")
        fh:flush()
    end,
})
