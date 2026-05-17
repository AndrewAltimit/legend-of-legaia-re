-- autorun_dump_full_ram.lua
--
-- Dump the full 2 MiB main RAM from a PCSX-Redux save state to disk. The
-- 2 MiB single readAt() permanently degrades subsequent vsync delivery
-- (see lib/probe.lua caveats), so this script does ONE dump and quits —
-- multi-snapshot probes should use autorun_boot_walk_snapshots.lua's
-- chunked-per-vsync pattern instead.
--
-- Env vars:
--   LEGAIA_SSTATE        save state path (default sstate2)
--   LEGAIA_OUT           output .bin path (default ram_full.bin)
--   LEGAIA_FRAMES        post-load settle vsyncs before dump (default 120)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2")
local OUT_PATH    = probe.out_path("ram_full.bin")
local SETTLE      = probe.getenv_num("LEGAIA_FRAMES", 120)

PCSX.log(string.format(
    "[dump_ram] sstate=%s out=%s size=%d settle=%d",
    SSTATE_PATH, OUT_PATH, probe.RAM_SIZE, SETTLE))

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = SETTLE,

    -- No breakpoints — the lib still runs the boot/load/settle state
    -- machine for us. on_arm just announces the wait.
    on_arm = function(_)
        PCSX.log(string.format(
            "[dump_ram] settling %d vsyncs before dump", SETTLE))
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
                "[dump_ram] FATAL: cannot open %s: %s",
                OUT_PATH, tostring(err)))
            return
        end
        fh:write(s)
        fh:close()
        PCSX.log(string.format(
            "[dump_ram] wrote %d bytes to %s", #s, OUT_PATH))
    end,
})
