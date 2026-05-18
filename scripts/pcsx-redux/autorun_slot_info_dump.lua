-- autorun_slot_info_dump.lua
--
-- Capture ground-truth pixels at the load-screen slot-info / slot-preview
-- state: load sstate9 (parked on the load screen), tap CROSS so the
-- slot picker enters the "Now checking" -> slot preview flow, settle a
-- few hundred vsyncs, then dump framebuffer + VRAM + main RAM.
--
-- Two captures are written:
--   * <out>/now_checking_fb.{raw,meta}   (after the X tap, while the
--                                         "Now checking" dialog is up)
--   * <out>/slot_info_fb.{raw,meta}      (later, after the dialog
--                                         clears and the portrait grid
--                                         + info panel is on screen)
--   * <out>/slot_info_vram.bin           VRAM at the slot-info moment
--   * <out>/slot_info_ram.bin            main RAM at the slot-info moment
--
-- We rely on probe.sm's hold_button hook to press CROSS for a small
-- number of vsyncs after the save state loads, then capture in
-- on_capture at two fixed vsync offsets.
--
-- Env vars:
--   LEGAIA_SSTATE   sstate path (default sstate9)
--   LEGAIA_OUT_DIR  output dir (default captures/slot_info_dump/<iso>)
--   LEGAIA_FRAMES   total post-load capture vsyncs (default 360)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate9")
local OUT_NOW_RAW  = probe.out_path("now_checking_fb.raw")
local OUT_NOW_META = probe.out_path("now_checking_fb.meta")
local OUT_SLOT_RAW  = probe.out_path("slot_info_fb.raw")
local OUT_SLOT_META = probe.out_path("slot_info_fb.meta")
local OUT_RAM_BIN = probe.out_path("slot_info_ram.bin")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 180)

-- Hold CROSS this many vsyncs after sstate load, then release.
local HOLD_FRAMES = 6
-- Capture the "Now checking" framebuffer at this vsync offset.
local NOW_AT      = 18
-- Capture the slot-info framebuffer + VRAM + RAM near the end.
local SLOT_AT     = math.max(FRAMES - 10, 90)

-- VRAM dump path (written alongside the slot-info framebuffer).
local OUT_VRAM_BIN = probe.out_path("slot_info_vram.bin")

PCSX.log(string.format(
    "[slot_info_dump] sstate=%s frames=%d now_at=%d slot_at=%d",
    SSTATE_PATH, FRAMES, NOW_AT, SLOT_AT))

local function take_fb(raw_path, meta_path, label)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil then
        PCSX.log(string.format("[slot_info_dump] %s takeScreenShot unavailable", label))
        return
    end
    local bpp_bits = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
    local bpx = bpp_bits / 8
    local w, h = tonumber(ss.width), tonumber(ss.height)
    local fh = io.open(raw_path, "wb")
    if fh ~= nil then
        local s = tostring(ss.data)
        fh:write(s); fh:close()
        PCSX.log(string.format("[slot_info_dump] %s fb: %dx%d %dbpp (%d bytes) -> %s",
            label, w, h, bpp_bits, #s, raw_path))
    end
    local mh = io.open(meta_path, "w")
    if mh ~= nil then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\nbytes_per_pixel=%d\n",
            w, h, bpp_bits, bpx))
        mh:close()
    end
end

local function dump_main_ram(path)
    local buf = probe.read_bytes(0x80000000, probe.RAM_SIZE)
    if buf == nil then
        PCSX.log("[slot_info_dump] main RAM read FAILED")
        return
    end
    local fh = io.open(path, "wb")
    if fh ~= nil then
        local s = tostring(buf); fh:write(s); fh:close()
        PCSX.log(string.format("[slot_info_dump] ram: %d bytes -> %s", #s, path))
    end
end

local function dump_vram(path)
    -- PCSX-Redux exposes VRAM via PCSX.GPU.vram which is a 1 MiB blob
    -- of BGR555 pixels (1024x512). The exact accessor name varies a
    -- bit by build; try the common ones in order.
    local ok, data = pcall(function()
        if PCSX.getVRAM ~= nil then return PCSX.getVRAM() end
        if PCSX.GPU and PCSX.GPU.getVRAM then return PCSX.GPU.getVRAM() end
        if PCSX.GPU and PCSX.GPU.vram then return PCSX.GPU.vram end
        return nil
    end)
    if not ok or data == nil then
        PCSX.log("[slot_info_dump] VRAM accessor unavailable; framebuffer .raw still captured")
        return
    end
    local fh = io.open(path, "wb")
    if fh ~= nil then
        local s = tostring(data); fh:write(s); fh:close()
        PCSX.log(string.format("[slot_info_dump] vram: %d bytes -> %s", #s, path))
    end
end

local captured_now = false
local captured_slot = false

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    hold_button    = probe.BTN.CROSS,
    hold_frames    = HOLD_FRAMES,

    on_arm = function(_) return {} end,

    on_capture = function(_, vsync_in_capture)
        if not captured_now and vsync_in_capture >= NOW_AT then
            captured_now = true
            take_fb(OUT_NOW_RAW, OUT_NOW_META, "now_checking")
        end
        if not captured_slot and vsync_in_capture >= SLOT_AT then
            captured_slot = true
            take_fb(OUT_SLOT_RAW, OUT_SLOT_META, "slot_info")
            dump_main_ram(OUT_RAM_BIN)
            dump_vram(OUT_VRAM_BIN)
        end
    end,

    on_done = function(_, _)
        -- belt-and-braces: if the on_capture path missed the slot
        -- frame for any reason, take one more snapshot at quit.
        if not captured_slot then
            take_fb(OUT_SLOT_RAW, OUT_SLOT_META, "slot_info_late")
            dump_main_ram(OUT_RAM_BIN)
            dump_vram(OUT_VRAM_BIN)
        end
    end,
})
