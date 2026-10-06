-- autorun_grey_ramp_writer.lua
--
-- Who writes the 31-step grey ramp at VRAM (451..481, 507)?
--
-- The ramp (`0x0421 * k`, k = 1..31) sits in every field state of the
-- save-state library from the publisher logos on, including teien's hedge
-- CLUT row, and no LoadImage runs on the row while a field is steady. So the
-- writer is at boot. This probe cold-boots the disc, breaks on the VRAM
-- upload routine `FUN_80059BD4` (the LoadImage body, `a0` = RECT*,
-- `a1` = source) and logs every upload whose rectangle covers (451, 507),
-- with `ra`, and polls VRAM once a vsync to stamp the frame the ramp first
-- appears.
--
-- Launch:
--   LEGAIA_NO_SSTATE=1 LEGAIA_FRAMES=1800 \
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_grey_ramp_writer.lua --frames 1800
--
-- Output: grey_ramp_writer.csv  tick,event,x,y,w,h,src,ra,detail

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local UPLOAD = 0x80059BD4
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1800)
local PX, PY = 451, 507
local CSV = probe.csv_open(probe.out_path("grey_ramp_writer.csv"),
    "tick,event,x,y,w,h,src,ra,detail")

require("probe.sstate").load = function(_)
    PCSX.log("[grey-ramp] cold boot; sstate load skipped")
    return true
end

local function get_vram()
    local ok, data = pcall(function()
        if PCSX.getVRAM ~= nil then return PCSX.getVRAM() end
        if PCSX.GPU and PCSX.GPU.getVRAM then return PCSX.GPU.getVRAM() end
        return nil
    end)
    if not ok or data == nil then return nil end
    return tostring(data)
end

local function px(vram, x, y)
    local o = (y * 1024 + x) * 2 + 1
    return vram:byte(o) + vram:byte(o + 1) * 256
end

local function rd_u16(addr)
    local b = probe.read_bytes(addr, 2)
    if b == nil then return -1 end
    local s = tostring(b)
    return s:byte(1) + s:byte(2) * 256
end

local tick, seen = 0, false

probe.run({
    sstate = "unused",
    capture_frames = FRAMES,
    on_arm = function()
        probe.arm_breakpoint(UPLOAD, "Exec", 4, "vram_upload", function()
            local r = PCSX.getRegisters()
            local rect = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
            local src = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
            local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            if not probe.in_ram(rect, 8) then return end
            local x, y = rd_u16(rect), rd_u16(rect + 2)
            local w, h = rd_u16(rect + 4), rd_u16(rect + 6)
            if PX >= x and PX < x + w and PY >= y and PY < y + h then
                local first = probe.in_ram(src, 4) and rd_u16(src) or -1
                CSV:row("%d,upload,%d,%d,%d,%d,0x%08X,0x%08X,src0=0x%04X",
                    tick, x, y, w, h, src, ra, first)
                PCSX.log(string.format(
                    "[grey-ramp] upload (%d,%d) %dx%d src=0x%08X ra=0x%08X",
                    x, y, w, h, src, ra))
            end
        end)
        return {}
    end,
    on_capture = function(_ctx, elapsed)
        tick = elapsed
        if seen then return end
        local v = get_vram()
        if v and px(v, PX, PY) == 0x0421 and px(v, PX + 1, PY) == 0x0842 then
            seen = true
            CSV:row("%d,ramp_present,%d,%d,0,0,0,0,mode=0x%X", elapsed, PX, PY,
                probe.read_u16(0x8007B83C) or -1)
            PCSX.log(string.format("[grey-ramp] ramp present at vsync %d", elapsed))
        end
    end,
    on_done = function()
        CSV:close()
    end,
})
