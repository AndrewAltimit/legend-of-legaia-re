-- autorun_dump_full_ram_hold.lua
--
-- Like autorun_dump_full_ram.lua, but holds a pad direction for the first
-- LEGAIA_HOLD vsyncs so a pre-transition save can drive its scene-change /
-- kingdom warp, then dumps the full 2 MiB main RAM AFTER the warp + load have
-- settled. Used to byte-locate a per-kingdom slot-4 resident base (the base
-- varies per kingdom; see docs/formats/world-map-overlay.md) by searching the
-- post-warp RAM for the disc-decoded slot-4 payload.
--
-- The 2 MiB single readAt() permanently degrades vsync delivery, so the dump is
-- the last thing the script does (on_done) and it quits immediately after.
--
-- Env vars:
--   LEGAIA_SSTATE        save state path
--   LEGAIA_OUT           output .bin path (default ram_full.bin)
--   LEGAIA_FRAMES        total capture vsyncs before the dump (default 240)
--   LEGAIA_HOLD_BUTTON   pad bit index to hold (UP=4, DOWN=6; 0 = none)
--   LEGAIA_HOLD          vsyncs to hold the button (default 0)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2")
local OUT_PATH    = probe.out_path("ram_full.bin")
local SETTLE      = probe.getenv_num("LEGAIA_FRAMES", 240)
local HOLD_BUTTON = probe.getenv_num("LEGAIA_HOLD_BUTTON", 0)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD", 0)

PCSX.log(string.format(
    "[dump_ram] sstate=%s out=%s size=%d settle=%d hold_btn=%d hold=%d",
    SSTATE_PATH, OUT_PATH, probe.RAM_SIZE, SETTLE, HOLD_BUTTON, HOLD_FRAMES))

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = SETTLE,
    hold_button    = HOLD_BUTTON ~= 0 and HOLD_BUTTON or nil,
    hold_frames    = HOLD_FRAMES,

    on_arm = function(_)
        PCSX.log(string.format(
            "[dump_ram] holding btn %d for %d vsyncs, settling %d total before dump",
            HOLD_BUTTON, HOLD_FRAMES, SETTLE))
        return {}
    end,

    on_done = function(_, _)
        local buf = probe.read_bytes(0x80000000, probe.RAM_SIZE)
        if buf == nil then
            PCSX.log("[dump_ram] FATAL: cannot read main RAM")
            return
        end
        local s = tostring(buf)
        local fh, err = io.open(OUT_PATH, "wb")
        if fh == nil then
            PCSX.log(string.format(
                "[dump_ram] FATAL: cannot open %s: %s", OUT_PATH, tostring(err)))
            return
        end
        fh:write(s)
        fh:close()
        PCSX.log(string.format(
            "[dump_ram] wrote %d bytes to %s", #s, OUT_PATH))
    end,
})
