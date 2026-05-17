-- autorun_load_screen_dump.lua
--
-- Capture ground-truth pixels at the load-screen state so we can
-- pin every sprite/position retail uses without guessing.
--
-- What it writes:
--   <out>/load_screen_fb.raw   raw BGR555 framebuffer bytes
--                              (or RGB24 if the GPU is in 24bpp mode)
--   <out>/load_screen_fb.meta  one line per metadata field
--                              (width, height, bpp, bytes_per_pixel)
--   <out>/load_screen_ram.bin  full 2 MiB main RAM at sstate9
--
-- After it runs, scripts/pcsx-redux/decode_load_screen.py converts
-- the .raw + .meta to a PNG and indexes pixel colors so we can
-- back out which source TIM each on-screen pixel comes from.
--
-- Env vars:
--   LEGAIA_SSTATE   sstate path (default sstate9 = parked on load
--                   screen per the engine note in legaia-backlog.txt)
--   LEGAIA_OUT_DIR  output dir (default captures/load_screen_dump/<iso>)
--   LEGAIA_FRAMES   settle vsyncs before capture (default 180)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate9")
local OUT_FB_RAW  = probe.out_path("load_screen_fb.raw")
local OUT_FB_META = probe.out_path("load_screen_fb.meta")
local OUT_RAM_BIN = probe.out_path("load_screen_ram.bin")
local SETTLE      = probe.getenv_num("LEGAIA_FRAMES", 180)

PCSX.log(string.format(
    "[load_screen_dump] sstate=%s fb=%s ram=%s settle=%d",
    SSTATE_PATH, OUT_FB_RAW, OUT_RAM_BIN, SETTLE))

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = SETTLE,

    on_arm = function(_)
        PCSX.log(string.format(
            "[load_screen_dump] settling %d vsyncs before capture", SETTLE))
        return {}
    end,

    on_done = function(_, _)
        -- 1) framebuffer screenshot — the rendered load-screen frame.
        local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
        if ok and ss ~= nil then
            local bpp = tonumber(ss.bpp) or 0
            local bpp_bits = 16
            -- ScreenShot::BPP_16 / BPP_24 are an enum; tostring sometimes
            -- yields raw int. Treat anything > 16 as 24bpp.
            if bpp > 16 then bpp_bits = 24 end
            local bpx = bpp_bits / 8
            local w, h = tonumber(ss.width), tonumber(ss.height)
            local fh, err = io.open(OUT_FB_RAW, "wb")
            if fh ~= nil then
                local s = tostring(ss.data)
                fh:write(s)
                fh:close()
                PCSX.log(string.format(
                    "[load_screen_dump] fb: %dx%d %dbpp (%d bytes) -> %s",
                    w, h, bpp_bits, #s, OUT_FB_RAW))
            else
                PCSX.log(string.format(
                    "[load_screen_dump] FB write FAILED: %s",
                    tostring(err)))
            end
            local mh = io.open(OUT_FB_META, "w")
            if mh ~= nil then
                mh:write(string.format(
                    "width=%d\nheight=%d\nbpp=%d\nbytes_per_pixel=%d\n",
                    w, h, bpp_bits, bpx))
                mh:close()
            end
        else
            PCSX.log("[load_screen_dump] takeScreenShot() unavailable")
        end

        -- 2) main RAM dump — useful for cross-referencing the GPU
        -- DMA command list pointer (DMA chan 2 base reg in scratch)
        -- and for locating any sprite-descriptor tables the title
        -- overlay built up before reaching the load-screen state.
        local buf = probe.read_bytes(0x80000000, probe.RAM_SIZE)
        if buf ~= nil then
            local fh, err = io.open(OUT_RAM_BIN, "wb")
            if fh ~= nil then
                local s = tostring(buf)
                fh:write(s)
                fh:close()
                PCSX.log(string.format(
                    "[load_screen_dump] ram: %d bytes -> %s",
                    #s, OUT_RAM_BIN))
            else
                PCSX.log(string.format(
                    "[load_screen_dump] RAM write FAILED: %s",
                    tostring(err)))
            end
        else
            PCSX.log("[load_screen_dump] main RAM read FAILED")
        end
    end,
})
