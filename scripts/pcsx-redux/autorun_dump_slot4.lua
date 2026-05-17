-- autorun_dump_slot4.lua
--
-- Dump the live slot-4 (world-map overlay outlines) RAM region from a
-- PCSX-Redux save state so we can byte-compare it against the disc-decoded
-- bytes. Produces ground-truth for the slot-4 verification scripts.
--
-- Env vars:
--   LEGAIA_SSTATE        save state path (default sstate2 = map-overview)
--   LEGAIA_KINGDOM       drake | sebucus | karisto (default drake)
--   LEGAIA_OUT           output .bin path (default slot4_ram.bin)
--   LEGAIA_FRAMES        post-load settle vsyncs (default 120)
--
-- The slot-4 base 0x8011A624 is pinned for Drake by the full-RAM
-- signature search in autorun_dump_full_ram.lua. Sebucus / Karisto
-- inherit the same base on the assumption that the kingdom loader
-- writes all three to the same slot. If a mismatch appears, dump full
-- RAM first and search for the 64-byte payload prefix.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate2")
local KINGDOM     = probe.getenv("LEGAIA_KINGDOM", "drake")
local OUT_PATH    = probe.out_path("slot4_ram.bin")
local SETTLE      = probe.getenv_num("LEGAIA_FRAMES", 120)

local KINGDOMS = {
    drake   = { base = 0x8011A624, size = 32304 },
    sebucus = { base = 0x8011A624, size = 26964 },
    karisto = { base = 0x8011A624, size = 24444 },
}

local cfg = KINGDOMS[KINGDOM]
if cfg == nil then
    PCSX.log(string.format(
        "[dump_slot4] FATAL: unknown kingdom '%s'", KINGDOM))
    PCSX.quit(2)
    return
end

PCSX.log(string.format(
    "[dump_slot4] sstate=%s kingdom=%s base=0x%08X size=%d out=%s",
    SSTATE_PATH, KINGDOM, cfg.base, cfg.size, OUT_PATH))

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = SETTLE,

    on_arm = function(_)
        PCSX.log(string.format(
            "[dump_slot4] settling %d vsyncs before dump", SETTLE))
        return {}
    end,

    on_done = function(_, _)
        local buf = probe.read_bytes(cfg.base, cfg.size)
        if buf == nil then
            PCSX.log(string.format(
                "[dump_slot4] FATAL: cannot read %d bytes at 0x%08X",
                cfg.size, cfg.base))
            return
        end
        local s = tostring(buf)
        local fh, err = io.open(OUT_PATH, "wb")
        if fh == nil then
            PCSX.log(string.format(
                "[dump_slot4] FATAL: cannot open %s: %s",
                OUT_PATH, tostring(err)))
            return
        end
        fh:write(s)
        fh:close()
        PCSX.log(string.format(
            "[dump_slot4] wrote %d bytes to %s", #s, OUT_PATH))

        -- Peek at the first 8 bytes to confirm the header looks valid.
        if #s >= 8 then
            local count = s:byte(1) + s:byte(2) * 256
                        + s:byte(3) * 65536 + s:byte(4) * 16777216
            local off0  = s:byte(5) + s:byte(6) * 256
                        + s:byte(7) * 65536 + s:byte(8) * 16777216
            PCSX.log(string.format(
                "[dump_slot4] header: count=%d  byte_offsets[0]=0x%X",
                count, off0))
        end
    end,
})
