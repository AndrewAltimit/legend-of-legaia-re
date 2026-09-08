-- probe/prim_scan.lua  -- find textured GPU primitives in main RAM by CLUT id.
--
-- The question "which routine draws the texture page at VRAM (X, Y)?" has no
-- static answer when neither the tpage id nor the CBA appears as an immediate
-- or a data halfword anywhere on the disc (the card-screen kanji page and the
-- init.pak WARNING screen are both in that state). What settles it is looking
-- at the primitives themselves: a textured PSX primitive carries its CLUT id
-- (CBA) in the HIGH halfword of packet word 3 and, for the poly kinds, its
-- tpage in the high halfword of word 6. Those halfwords are assembled at
-- runtime, so they exist in RAM even when no image spells them out.
--
-- This module snapshots main RAM once and scans it for a given CLUT halfword,
-- validating each candidate as a packet by its GP0 command byte. It is a
-- POSITIVE-CONTROL instrument: scan for a CLUT you KNOW is drawn (a publisher
-- logo) in the same pass as the one you are hunting, and a zero-hit result on
-- the hunted CLUT is evidence rather than a silence you cannot interpret.
--
-- Packet layout the scan relies on (identical for POLY_*T* and SPRT):
--
--     word0  packet+0    tag       (len << 24 | next)
--     word1  packet+4    code << 24 | bgr0      <- command byte validated here
--     word2  packet+8    y << 16 | x
--     word3  packet+12   clut << 16 | v0 << 8 | u0   <- the CLUT halfword
--     word6  packet+24   tpage << 16 | v1 << 8 | u1  (poly kinds only)
--
-- Usage:
--   local scan = require("probe.prim_scan")
--   local ram  = scan.snapshot()
--   for _, h in ipairs(scan.find_clut(ram, 0x7E80, 8)) do
--       PCSX.log(string.format("packet 0x%08X code=0x%02X x=%d y=%d tpage=0x%04X",
--                              h.packet_va, h.code, h.x, h.y, h.tpage))
--   end
--
-- Cost: one 2 MiB read plus one C-level string.find sweep per CLUT. Cheap
-- enough to run every N vsyncs; too expensive to run per breakpoint hit.

local mem = require("probe.mem")

local M = {}

local RAM_BASE = 0x80000000

-- GP0 command bytes that carry a CLUT in word 3. Textured polys
-- (0x24..0x27, 0x2C..0x2F, 0x34..0x37, 0x3C..0x3F) and textured rects
-- (0x64..0x67, 0x6C..0x6F, 0x74..0x77, 0x7C..0x7F). The low two bits are the
-- raw-texture / semi-transparency selects, so each family spans four codes.
local TEXTURED = {}
for _, base in ipairs({ 0x24, 0x2C, 0x34, 0x3C, 0x64, 0x6C, 0x74, 0x7C }) do
    for i = 0, 3 do TEXTURED[base + i] = true end
end

-- Command bytes whose packet has a word 6 (the poly kinds - a rect packet is
-- only five words long, so word 6 of a SPRT belongs to the next packet).
local HAS_TPAGE = {}
for _, base in ipairs({ 0x24, 0x2C, 0x34, 0x3C }) do
    for i = 0, 3 do HAS_TPAGE[base + i] = true end
end

M.TEXTURED_CODES = TEXTURED

-- A two-byte CLUT/tpage needle matches ordinary data as often as it matches a
-- packet: over 2 MiB of RAM a given halfword lands on a word-aligned offset
-- behind a byte that happens to be a textured GP0 code a few times per sweep.
-- Those false positives are separable from real primitives by their SCREEN
-- COORDINATES: the GPU takes signed 11-bit x/y, so a real packet's word 2
-- decodes inside -1024..1023, while a random word decodes to five-digit
-- garbage. Measured on a cold boot: every publisher-logo packet landed on its
-- documented screen rect, and every hit for a CLUT that is NOT drawn had
-- out-of-range coordinates. Treat `plausible` as the signal and the raw hit
-- list as the audit trail - do NOT filter in the scanner, or a real draw at an
-- odd coordinate would vanish silently.
local function coord_ok(v)
    return v <= 1023 or v >= 0xFC00
end

function M.plausible(hit)
    return coord_ok(hit.x) and coord_ok(hit.y)
end

-- Read all of main RAM as a Lua string. Returns nil if the read fails.
function M.snapshot()
    local buf = mem.read_bytes(RAM_BASE, mem.RAM_SIZE)
    if buf == nil then return nil end
    return tostring(buf)
end

-- Find every word-aligned textured-primitive packet in `ram` whose word-3
-- CLUT halfword equals `clut`. Returns a list of tables:
--   { packet_va, word3_va, code, x, y, u, v, tpage }
-- `max_hits` caps the list (default 32); the scan still runs to completion so
-- `total` (second return) reports how many candidates matched.
function M.find_clut(ram, clut, max_hits)
    max_hits = max_hits or 32
    local hits, total = {}, 0
    if ram == nil then return hits, 0 end

    local lo = bit.band(clut, 0xFF)
    local hi = bit.band(bit.rshift(clut, 8), 0xFF)
    local needle = string.char(lo, hi)

    local init = 1
    while true do
        local i = string.find(ram, needle, init, true)
        if i == nil then break end
        init = i + 1

        -- `i` is the 1-based index of the CLUT's low byte, which sits at
        -- byte 2 of word 3. So word 3 starts at 0-based offset i - 3.
        local w3 = i - 3
        if w3 >= 8 and (w3 % 4) == 0 then
            -- word1's command byte is its MSB: 0-based (w3 - 8) + 3.
            local code = ram:byte(w3 - 4)
            if code ~= nil and TEXTURED[code] then
                total = total + 1
                if #hits < max_hits then
                    local x = ram:byte(w3 - 3) + bit.lshift(ram:byte(w3 - 2), 8)
                    local y = ram:byte(w3 - 1) + bit.lshift(ram:byte(w3), 8)
                    local tpage = nil
                    if HAS_TPAGE[code] and (w3 + 16) <= #ram then
                        tpage = ram:byte(w3 + 15) + bit.lshift(ram:byte(w3 + 16), 8)
                    end
                    hits[#hits + 1] = {
                        packet_va = RAM_BASE + w3 - 12,
                        word3_va  = RAM_BASE + w3,
                        code      = code,
                        x         = x,
                        y         = y,
                        u         = ram:byte(w3 + 1),
                        v         = ram:byte(w3 + 2),
                        tpage     = tpage,
                    }
                end
            end
        end
    end
    return hits, total
end

-- Find every word-aligned packet whose word-6 TPAGE halfword equals `tpage`,
-- for the poly kinds only (a rect packet has no word 6). Same hit shape as
-- find_clut. Use this when the hunted page is addressed by tpage rather than
-- by CLUT - a 16bpp still, say, which has no CLUT at all.
function M.find_tpage(ram, tpage, max_hits)
    max_hits = max_hits or 32
    local hits, total = {}, 0
    if ram == nil then return hits, 0 end

    local needle = string.char(bit.band(tpage, 0xFF),
                               bit.band(bit.rshift(tpage, 8), 0xFF))
    local init = 1
    while true do
        local i = string.find(ram, needle, init, true)
        if i == nil then break end
        init = i + 1
        -- The tpage halfword sits at byte 2 of word 6, so word 3 (whose
        -- command byte we validate against) starts 12 bytes earlier.
        local w6 = i - 3
        local w3 = w6 - 12
        if w3 >= 8 and (w3 % 4) == 0 then
            local code = ram:byte(w3 - 4)
            if code ~= nil and HAS_TPAGE[code] then
                total = total + 1
                if #hits < max_hits then
                    hits[#hits + 1] = {
                        packet_va = RAM_BASE + w3 - 12,
                        word3_va  = RAM_BASE + w3,
                        code      = code,
                        x         = ram:byte(w3 - 3) + bit.lshift(ram:byte(w3 - 2), 8),
                        y         = ram:byte(w3 - 1) + bit.lshift(ram:byte(w3), 8),
                        clut      = ram:byte(w3 + 3) + bit.lshift(ram:byte(w3 + 4), 8),
                        tpage     = tpage,
                    }
                end
            end
        end
    end
    return hits, total
end

-- Find every word-aligned packet whose word 2 is exactly `(y << 16) | x` and
-- whose command byte is in `codes` (a set keyed by byte value). Word 2 is the
-- first vertex for a poly, the top-left for a rect, and the SOURCE corner for a
-- VRAM-to-VRAM blit (GP0 `0x80`), so this is how you ask "does anything address
-- the page at (384, 0)?" without knowing which primitive kind would do it.
-- Returns { packet_va, word2_va, code } hits plus the candidate total.
function M.find_at_xy(ram, x, y, codes, max_hits)
    max_hits = max_hits or 32
    local hits, total = {}, 0
    if ram == nil then return hits, 0 end

    local needle = string.char(bit.band(x, 0xFF), bit.band(bit.rshift(x, 8), 0xFF),
                               bit.band(y, 0xFF), bit.band(bit.rshift(y, 8), 0xFF))
    local init = 1
    while true do
        local i = string.find(ram, needle, init, true)
        if i == nil then break end
        init = i + 1
        local w2 = i - 1
        if w2 >= 8 and (w2 % 4) == 0 then
            local code = ram:byte(w2)      -- word1's MSB: 0-based (w2-4)+3
            local ok = code ~= nil and codes[code]
            -- A KSEG0 POINTER IS A FALSE 0x80 PACKET. Any word `0x80xxxxxx`
            -- followed by the hunted coordinate word decodes as a "GP0 0x80
            -- blit at (x, y)", and main RAM is full of `0x80`-prefixed
            -- pointers - the first version of this scan reported a steady
            -- stream of them. libgpu writes a blit's command word as exactly
            -- `code << 24` (DR_MOVE sets `code[0] = 0x80000000`), so demand
            -- that the low 24 bits are zero for the blit family, and demand a
            -- transferable size. The textured families carry `bgr` in those
            -- bits and cannot be filtered this way.
            if ok and code >= 0x80 and code <= 0x83 then
              if (w2 + 12) > #ram then
                ok = false
              else
                local payload = ram:byte(w2 - 3)
                              + bit.lshift(ram:byte(w2 - 2), 8)
                              + bit.lshift(ram:byte(w2 - 1), 16)
                local sw = ram:byte(w2 + 9) + bit.lshift(ram:byte(w2 + 10), 8)
                local sh = ram:byte(w2 + 11) + bit.lshift(ram:byte(w2 + 12), 8)
                ok = payload == 0 and sw >= 1 and sw <= 1024 and sh >= 1 and sh <= 512
              end
            end
            if ok then
                total = total + 1
                if #hits < max_hits then
                    -- A GP0 0x80 blit is [tag][code][src][dst][h<<16|w], so
                    -- the two words after word 2 tell a real move from a
                    -- coincidence: report them rather than making the caller
                    -- guess from an address alone.
                    local function w16(off)
                        if (w2 + off + 2) > #ram then return nil end
                        return ram:byte(w2 + off + 1) + bit.lshift(ram:byte(w2 + off + 2), 8)
                    end
                    hits[#hits + 1] = {
                        packet_va = RAM_BASE + w2 - 8,
                        word2_va  = RAM_BASE + w2,
                        code      = code,
                        dst_x     = w16(4),
                        dst_y     = w16(6),
                        size_w    = w16(8),
                        size_h    = w16(10),
                    }
                end
            end
        end
    end
    return hits, total
end

-- GP0 `0x80..0x83` - VRAM-to-VRAM blit. A full-screen still is as likely to be
-- moved into the display area as it is to be textured off, so a "who draws
-- (384, 0)?" sweep that only looks at textured primitives answers half the
-- question.
M.MOVE_IMAGE_CODES = { [0x80] = true, [0x81] = true, [0x82] = true, [0x83] = true }

-- Find a libgpu RECT / DISPENV whose origin is (x, y) and whose size is a
-- plausible screen. A full-screen VRAM still is usually SHOWN by pointing the
-- display area at it, not by drawing a primitive, and a DISPENV is
-- `short x, y, w, h` - so this is the other half of "who displays (384, 0)".
-- Returns { va, w, h } hits.
function M.find_disp_rect(ram, x, y, max_hits)
    max_hits = max_hits or 32
    local hits = {}
    if ram == nil then return hits end

    local needle = string.char(bit.band(x, 0xFF), bit.band(bit.rshift(x, 8), 0xFF),
                               bit.band(y, 0xFF), bit.band(bit.rshift(y, 8), 0xFF))
    local init = 1
    while true do
        local i = string.find(ram, needle, init, true)
        if i == nil then break end
        init = i + 1
        local o = i - 1                      -- 0-based offset of the RECT
        if (o % 4) == 0 and (o + 8) <= #ram then
            local w = ram:byte(o + 5) + bit.lshift(ram:byte(o + 6), 8)
            local h = ram:byte(o + 7) + bit.lshift(ram:byte(o + 8), 8)
            if w >= 256 and w <= 640 and h >= 224 and h <= 512 then
                if #hits < max_hits then
                    hits[#hits + 1] = { va = RAM_BASE + o, w = w, h = h }
                end
            end
        end
    end
    return hits
end

-- Convenience: scan one RAM snapshot for a whole table of labelled CLUTs.
-- `wanted` is { { clut = 0x7E80, label = "WARNING" }, ... }. Calls
-- `emit(label, clut, hit, total)` once per hit (and once with hit = nil when a
-- CLUT matched nothing, so the caller can record the zero).
function M.sweep(ram, wanted, emit, max_hits)
    for _, w in ipairs(wanted) do
        local hits, total = M.find_clut(ram, w.clut, max_hits)
        if total == 0 then
            emit(w.label, w.clut, nil, 0)
        else
            for _, h in ipairs(hits) do emit(w.label, w.clut, h, total) end
        end
    end
end

return M
