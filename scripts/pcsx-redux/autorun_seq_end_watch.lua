-- autorun_seq_end_watch.lua
--
-- What a score with no counted loop does at its end. Makes the field-VM
-- op-0x35 sub-op 1 start (the track word 0x8007BAC8 = LEGAIA_BGM_ID) and logs
-- every 30 vsyncs each open libsnd sequence: read cursor, pass counter `+0x21`,
-- loop count `+0x20`, flags `+0x98` (0x1 playing, 0x4 stopped, 0x200 finished).
--
-- Run:
--   LEGAIA_BGM_ID=2050 LEGAIA_FRAMES=2400 bash scripts/pcsx-redux/run_probe.sh \
--       --scenario s3_rimelm_freeroam \
--       --lua scripts/pcsx-redux/autorun_seq_end_watch.lua --out <path>.csv

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 2400)
local TRIG_V = probe.getenv_num("LEGAIA_TRIGGER_V", 60)
local BGM_ID = probe.getenv_num("LEGAIA_BGM_ID", 2050)
local OUT_PATH = probe.out_path("seq_end_watch.csv")
local csv = probe.csv_open(OUT_PATH, "vsync,word,slot,cursor,start,lcount,passes,flags")

local function n32(v) return (tonumber(v) or 0) % 0x100000000 end

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    boot_delay     = 60,
    snapshot_every = 1000000,
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        if elapsed == TRIG_V then probe.write_u32(0x8007BAC8, BGM_ID) end
        if elapsed % 30 == 0 and csv then
            local bitmap = probe.read_u16(0x801CD2B8)
            for s = 0, 15 do
                if bit.band(bitmap, bit.lshift(1, s)) ~= 0 then
                    local r = n32(probe.read_u32(0x801CD2C0 + s * 4))
                    if probe.in_ram(r) then
                        csv:row("%d,%d,%d,%08X,%08X,%d,%d,%04X", elapsed,
                            probe.read_u16(0x8007BAC8), s, n32(probe.read_u32(r)),
                            n32(probe.read_u32(r + 4)), probe.read_u8(r + 0x20),
                            probe.read_u8(r + 0x21), probe.read_u16(r + 0x98))
                    end
                end
            end
        end
        if elapsed >= FRAMES then ctx.request_quit = true end
    end,
})
