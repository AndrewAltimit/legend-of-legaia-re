-- autorun_vignette_panel_film.lua
--
-- Film the ending vignettes per vsync and log, beside each frame, the
-- screen-effect widgets on the actor lists - the image panel (handler
-- 0x801F849C: current, target, clock / duration), the mask (0x801F811C) and
-- every sprite (0x801F7A9C: position, clock, script cursor) - with the frame
-- step bytes 0x1F800392 / 0x1F800393 and every engaged field context's
-- record PC and wait (flat:pc:wait). Poll-only, safe to run --fast.
--
-- Env:
--   LEGAIA_FRAMES      vsyncs to run after load (default 4000)
--   LEGAIA_SHOT_EVERY  vsyncs between screenshots (default 1)
--   LEGAIA_SHOT_FROM / LEGAIA_SHOT_TO   the filmed window (default all)
--
-- Output: vignette_trace.csv and shot_<vsync>.raw (+ .meta) in the out dir.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh --fast --headless \
--       --iso <disc> --scenario credits_edstati3_panel_pcsx \
--       --lua scripts/pcsx-redux/autorun_vignette_panel_film.lua \
--       --out-dir <dir> --frames 3000
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 4000)
local EVERY = probe.getenv_num("LEGAIA_SHOT_EVERY", 1)
local FROM = probe.getenv_num("LEGAIA_SHOT_FROM", 0)
local TO = probe.getenv_num("LEGAIA_SHOT_TO", 999999)

local function s16(a)
    local v = probe.read_u16(a) or 0
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return false end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return false end
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 240,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
    return true
end

local fh = assert(io.open(probe.out_path("vignette_trace.csv"), "w"))
fh:write("vsync,step392,step393,sprites,panel,mask,ctx\n")

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_vignette_panel_film.lua",
            { sstate = SSTATE, frames = FRAMES, every = EVERY })
        return {}
    end,
    on_capture = function(_, el)
        local panel, mask, ctx, spr = {}, {}, {}, {}
        local seen = {}
        for k = 0, 8 do
            local n, count = probe.read_u32(0x8007C34C + 4 * k), 0
            while probe.in_ram(n) and count < 512 and not seen[n] do
                seen[n] = true
                count = count + 1
                local h = probe.read_u32(n + 0x0C)
                if h == 0x801F849C then
                    panel[#panel + 1] = string.format("%d %d %d %d %d|%d %d %d %d %d|t%d/%d",
                        s16(n + 0x14), s16(n + 0x16), s16(n + 0x18), s16(n + 0x1A), s16(n + 0x24),
                        s16(n + 0x3C), s16(n + 0x3E), s16(n + 0x40), s16(n + 0x42), s16(n + 0x26),
                        s16(n + 0x9C), s16(n + 0x9E))
                elseif h == 0x801F7A9C then
                    spr[#spr + 1] = string.format("%d %d t%d c%X", s16(n + 0x14), s16(n + 0x16),
                        s16(n + 0x9C), (probe.read_u32(n + 0x90) or 0) % 0x10000)
                elseif h == 0x801F811C then
                    mask[#mask + 1] = string.format("%d %d %d %d|%d %d %d %d|t%d/%d",
                        s16(n + 0x14), s16(n + 0x16), s16(n + 0x18), s16(n + 0x1A),
                        s16(n + 0x3C), s16(n + 0x3E), s16(n + 0x40), s16(n + 0x42),
                        s16(n + 0x9C), s16(n + 0x9E))
                elseif h == 0x8003BC08 then
                    local f = probe.read_u32(n + 0x10) or 0
                    if bit.band(f, 0x100) ~= 0 then
                        ctx[#ctx + 1] = string.format("%d:%X:%d", probe.read_u16(n + 0x50) or 0,
                            probe.read_u16(n + 0x9E) or 0, s16(n + 0x54))
                    end
                end
                n = probe.read_u32(n)
            end
        end
        fh:write(string.format("%d,%d,%d,%s,%s,%s,%s\n", el,
            probe.read_scratch_u8(0x1F800392) or -1, probe.read_scratch_u8(0x1F800393) or -1,
            table.concat(spr, ";"), table.concat(panel, ";"), table.concat(mask, ";"), table.concat(ctx, ";")))
        if el % 60 == 0 then fh:flush() end
        if el >= FROM and el <= TO and (el - FROM) % EVERY == 0 then
            screenshot(string.format("shot_%05d", el))
        end
    end,
})
