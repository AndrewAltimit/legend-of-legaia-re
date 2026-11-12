-- probe/gpu_cost.lua  -- what one retail ordering table costs the GPU to draw.
--
-- Walks a DMA2 linked list (the table `DrawOTag` hands the GPU) and prices
-- each GP0 command on the v2 GPU's measured costs as DuckStation's
-- `src/core/gpu.cpp` records them (`AddDrawTriangleTicks`,
-- `AddDrawRectangleTicks`, `AddDrawLineTicks`, the polygon setup table and the
-- VRAM fill), and each list node on its DMA costs (`src/core/dma.cpp`: a
-- header read of 8, a block setup of 5 and a cycle a word). Every figure is in
-- CPU cycles (33.8688 MHz); one NTSC scanline is 3413 video cycles, so
-- 3413 * 7 / 11 CPU cycles (psx-spx: the video clock is the CPU's * 11 / 7).
-- Nothing here is fitted to a capture. `scripts/mednafen/gpu_cost.py` is the
-- same model over a save state's RAM; see docs/subsystems/battle-stage-camera.md,
-- "What a real console's frame costs".
--
-- PCSX-Redux draws for free (`gp+0x3A8` and `gp+0x3AC` agree), so on this
-- emulator the figure is a prediction of the GPU's share, not a reading.
--
-- Usage:
--   local gpu_cost = require("probe.gpu_cost")
--   local draw, dma, polys = gpu_cost.drawn_table()   -- in hblanks

local ffi = require("ffi")

local M = {}

M.HBLANK_CPU = 3413 * 7 / 11

local ram = nil
local function w32(off)
    if ram == nil then ram = ffi.cast("uint32_t*", PCSX.getMemPtr()) end
    return tonumber(ram[bit.rshift(off, 2)])
end

-- [quad][shaded][textured]
local SETUP = { [0] = { [0] = { [0] = 50, [1] = 130 }, [1] = { [0] = 200, [1] = 295 } },
                [1] = { [0] = { [0] = 50, [1] = 215 }, [1] = { [0] = 335, [1] = 495 } } }

local function s11(v)
    v = bit.band(v, 0x7FF)
    if v >= 0x400 then return v - 0x800 end
    return v
end

local A = { x0 = 0, y0 = 0, x1 = 319, y1 = 239, ox = 0, oy = 0, tmode = 0, mask = false }

local function clampv(x, y)
    x = math.min(math.max(x + A.ox, A.x0), A.x1 + 1)
    y = math.min(math.max(y + A.oy, A.y0), A.y1 + 1)
    return x, y
end

local function tri(ax, ay, bx, by, cx, cy, shaded, textured, semi)
    ax, ay = clampv(ax, ay); bx, by = clampv(bx, by); cx, cy = clampv(cx, cy)
    local pix = math.abs(ax * by + bx * cy + cx * ay - ax * cy - bx * ay - cx * by)
    pix = math.floor(pix / 2)
    local rows = math.max(ay, by, cy) - math.min(ay, by, cy)
    semi = semi or A.mask
    local flat = not shaded and not textured
    local q = flat and (semi and 3 or 2) or 4
    local rt = 2 + (semi and (flat and 3 or 1) or 0)
    return math.floor((pix * q + 3) / 4) + rows * rt
end

local function rect(x, y, w, h, textured, semi)
    local x0 = math.max(x + A.ox, A.x0); local y0 = math.max(y + A.oy, A.y0)
    local x1 = math.min(x + A.ox + w, A.x1 + 1); local y1 = math.min(y + A.oy + h, A.y1 + 1)
    local dw = math.max(0, x1 - x0); local dh = math.max(0, y1 - y0)
    semi = semi or A.mask
    local ppr = semi and dw or bit.band(dw, bit.bnot(1))
    local e = (semi and 64 or 16) + ppr * (semi and 6 or 4)
    if textured and dw > 0 then
        if A.tmode == 0 then
            e = e + dw * 6
        elseif A.tmode == 1 then
            if dw > 128 then e = e + dw * 11
            elseif dw * dh > 2048 then e = e + math.floor((math.floor(dw / 4) * (4 * math.floor(128 / dw))) * 11 / 2)
            else e = e + math.floor(dw * 11 / 2) end
        else
            if dw > 128 then e = e + dw * 20
            elseif dw * dh > 1024 then e = e + (math.floor(dw / 4) * (8 * math.floor(128 / dw))) * 5
            else e = e + dw * 5 end
        end
    end
    if dw > 0 and dw < 8 then e = math.max(e, 36) end
    return 50 + math.floor((e * dh + 7) / 8)
end

-- The GP0 commands of one node's `n` words at main-RAM offset `off`.
local function packet(off, n)
    local t, polys, i = 0, 0, 0
    while i < n do
        local w0 = w32(off + i * 4)
        local c = bit.rshift(w0, 24)
        if c >= 0x20 and c <= 0x3F then
            local quad = bit.band(c, 8) ~= 0
            local shaded = bit.band(c, 0x10) ~= 0
            local tex = bit.band(c, 4) ~= 0
            local semi = bit.band(c, 2) ~= 0
            local nv = quad and 4 or 3
            local per = 1 + (tex and 1 or 0) + (shaded and 1 or 0)
            local len = 1 + nv * per - (shaded and 1 or 0)
            if i + len > n then break end
            local vx, vy = {}, {}
            local j = i + 1
            for k = 1, nv do
                if shaded and k > 1 then j = j + 1 end
                local v = w32(off + j * 4)
                vx[k] = s11(v); vy[k] = s11(bit.rshift(v, 16)); j = j + 1
                if tex then
                    if k == 2 then A.tmode = bit.band(bit.rshift(w32(off + j * 4), 23), 3) end
                    j = j + 1
                end
            end
            t = t + SETUP[quad and 1 or 0][shaded and 1 or 0][tex and 1 or 0]
            t = t + tri(vx[1], vy[1], vx[2], vy[2], vx[3], vy[3], shaded, tex, semi)
            if quad then t = t + tri(vx[2], vy[2], vx[3], vy[3], vx[4], vy[4], shaded, tex, semi) end
            polys = polys + 1
            i = i + len
        elseif c >= 0x60 and c <= 0x7F then
            local tex = bit.band(c, 4) ~= 0
            local semi = bit.band(c, 2) ~= 0
            local size = bit.band(bit.rshift(c, 3), 3)
            local len = 2 + (tex and 1 or 0) + (size == 0 and 1 or 0)
            if i + len > n then break end
            local v = w32(off + (i + 1) * 4)
            local ww, hh
            if size == 0 then
                local wh = w32(off + (i + 2 + (tex and 1 or 0)) * 4)
                ww = bit.band(wh, 0x3FF); hh = bit.band(bit.rshift(wh, 16), 0x1FF)
            else
                ww = ({ 1, 8, 16 })[size]; hh = ww
            end
            t = t + rect(s11(v), s11(bit.rshift(v, 16)), ww, hh, tex, semi)
            i = i + len
        elseif c >= 0x40 and c <= 0x5F then
            local shaded = bit.band(c, 0x10) ~= 0
            local semi = bit.band(c, 2) ~= 0
            local poly = bit.band(c, 8) ~= 0
            local j, px, py = i + 1, {}, {}
            while j < n do
                if shaded and #px > 0 then j = j + 1 end
                if j >= n then break end
                local v = w32(off + j * 4)
                if poly and #px > 0 and bit.band(v, 0xF000F000) == 0x50005000 then j = j + 1; break end
                px[#px + 1] = s11(v); py[#py + 1] = s11(bit.rshift(v, 16)); j = j + 1
                if not poly and #px == 2 then break end
            end
            for k = 2, #px do
                local dw = math.abs(px[k] - px[k - 1]); local dh = math.abs(py[k] - py[k - 1])
                t = t + (shaded and 124 or 60) + math.floor((math.max(dw, dh) * 4 + dh * (semi and 26 or 16) + 3) / 4)
            end
            i = j
        elseif c == 0x02 then
            if i + 3 > n then break end
            local wh = w32(off + (i + 2) * 4)
            local ww = bit.band(bit.band(wh, 0x3FF) + 0xF, bit.bnot(0xF))
            local hh = bit.band(bit.rshift(wh, 16), 0x1FF)
            t = t + 10 + 5 * hh + math.floor(ww * hh / 12)
            i = i + 3
        elseif c == 0xE1 then A.tmode = bit.band(bit.rshift(w0, 7), 3); t = t + 1; i = i + 1
        elseif c == 0xE3 then A.x0 = bit.band(w0, 0x3FF); A.y0 = bit.band(bit.rshift(w0, 10), 0x1FF); t = t + 1; i = i + 1
        elseif c == 0xE4 then A.x1 = bit.band(w0, 0x3FF); A.y1 = bit.band(bit.rshift(w0, 10), 0x1FF); t = t + 1; i = i + 1
        elseif c == 0xE5 then A.ox = s11(w0); A.oy = s11(bit.rshift(w0, 11)); t = t + 1; i = i + 1
        elseif c == 0xE6 then A.mask = bit.band(w0, 2) ~= 0; t = t + 1; i = i + 1
        elseif c == 0x80 or c == 0xA0 or c == 0xC0 then break
        else i = i + 1 end
    end
    return t, polys
end

-- The list from `start` (a KSEG address): draw cycles, DMA cycles, polygons.
function M.list(start)
    A.x0, A.y0, A.x1, A.y1, A.ox, A.oy, A.tmode, A.mask = 0, 0, 319, 239, 0, 0, 0, false
    local draw, dma, polys = 0, 0, 0
    local off = bit.band(start, 0x1FFFFF)
    for _ = 1, 40000 do
        if off + 4 > 0x200000 then break end
        local h = w32(off)
        local cnt = bit.rshift(h, 24)
        local nxt = bit.band(h, 0xFFFFFF)
        dma = dma + 8 + (cnt > 0 and (5 + cnt) or 0)
        if cnt > 0 then
            local t, p = packet(off + 4, cnt)
            draw = draw + t; polys = polys + p
        end
        if nxt == 0xFFFFFF or bit.band(nxt, 0x800000) ~= 0 then break end
        off = bit.band(nxt, 0x1FFFFC)
    end
    return draw, dma, polys
end

-- The table the GPU was handed last: the record the frame-end flip left
-- behind the current one (`gp+0x434`), from its last bucket
-- (`DrawOTag(ot + len * 4 - 4)`, `0x80017264..0x80017288`; `len` is the
-- halfword at `0x1F8003A6`). In hblanks.
function M.drawn_table(read_u16, read_scratch_u16)
    local idx = bit.band(read_u16(0x8007B74C) or 0, 1)
    local base = w32(0x7BF30 + (1 - idx) * 0x74 + 0x70)
    local len = read_scratch_u16 and read_scratch_u16(0x1F8003A6) or 4096
    if len == nil or len == 0 then len = 4096 end
    if base < 0x80000000 or base >= 0x80200000 then return 0, 0, 0 end
    local draw, dma, polys = M.list(base + len * 4 - 4)
    return draw / M.HBLANK_CPU, dma / M.HBLANK_CPU, polys
end

return M
