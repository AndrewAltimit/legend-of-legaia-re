-- autorun_dance_countin_frame.lua
--
-- One retail frame of the dance minigame's pre-song count-in, caught on the
-- banner animator's own clock.
--
-- `FUN_801D2D98` (dance overlay PROT 0980, slot-A base `0x801CE818`, file
-- `+0x4580`) is called once per frame with the banner's frame counter in
-- `$a0`, and it emits through the hub sprite emitter `FUN_801D2F38` - two
-- half-brightness halves at widget `0x77` while the banner slides, one
-- full-brightness banner at widget `0x78` while it holds. No state in the
-- catalogue sits inside that window, so the frame has to be run to.
--
-- The probe arms the animator, logs its `$a0` (the envelope's whole input) per
-- call, and dumps the full 2 MiB main RAM on the `LEGAIA_HIT_AT`-th call so
-- the frame's ordering table can be decoded offline:
--
--     mednafen-state display-list --list <out>/ram_full.bin
--
-- The 2 MiB read permanently degrades vsync delivery, so it happens in
-- `on_done` and the run quits straight after - the capture is requested from
-- the breakpoint and taken one vsync later, so the dumped frame is the hit's
-- frame or its successor, not an arbitrary one.
--
-- Env:
--   LEGAIA_HIT_AT   animator call to dump on (default 12, mid slide-in)
--   LEGAIA_FRAMES   cap on capture vsyncs (default 3000)
--   LEGAIA_LABEL    free-text label for manifest.txt
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE  = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES  = probe.getenv_num("LEGAIA_FRAMES", 3000)
local HIT_AT  = probe.getenv_num("LEGAIA_HIT_AT", 12)
local LABEL   = probe.getenv("LEGAIA_LABEL", "dance-countin")

local ANIMATOR = 0x801D2D98
local EMITTER  = 0x801D2F38

local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function regs()
    local r = PCSX.getRegisters()
    return (r.GPR and r.GPR.n) or {}
end

local csv
local elapsed_now, hits, emits = 0, 0, 0
local dumped = false

probe.run({
    sstate = SSTATE, capture_frames = FRAMES,
    on_arm = function(ctx)
        PCSX.log(string.format("== dance count-in == label=%s hit_at=%d", LABEL, HIT_AT))
        probe.env.write_manifest("autorun_dance_countin_frame.lua", {
            label = LABEL, sstate = SSTATE, frames = FRAMES, hit_at = HIT_AT,
        })
        csv = probe.csv_open(probe.out_path("countin.csv"),
            "vsync,hit,kind,frame_or_x,widget,record,brightness,ra")
        probe.arm_breakpoint(ANIMATOR, "Exec", 4, "animator", function()
            local n = regs()
            hits = hits + 1
            csv:row("%d,%d,animate,%d,,,,0x%08X",
                elapsed_now, hits, tou32(n.a0), tou32(n.ra))
            if hits >= HIT_AT and not dumped then
                dumped = true
                ctx.request_quit = true
            end
        end)
        probe.arm_breakpoint(EMITTER, "Exec", 4, "emitter", function()
            local n = regs()
            emits = emits + 1
            csv:row("%d,%d,emit,%d,%d,%d,%d,0x%08X",
                elapsed_now, hits, tou32(n.a0), tou32(n.a1),
                tou32(n.a2), tou32(n.a3), tou32(n.ra))
        end)
        return {}
    end,
    on_capture = function(_, elapsed)
        elapsed_now = elapsed
    end,
    on_done = function()
        PCSX.log(string.format("[countin] animator=%d emits=%d dumped=%s",
            hits, emits, tostring(dumped)))
        if not dumped then return end
        local buf = probe.read_bytes(0x80000000, probe.RAM_SIZE)
        if buf == nil then
            PCSX.log("[countin] cannot read main RAM")
            return
        end
        local path = probe.out_path("ram_full.bin")
        local fh = io.open(path, "wb")
        if fh == nil then
            PCSX.log("[countin] cannot open " .. path)
            return
        end
        fh:write(tostring(buf))
        fh:close()
        PCSX.log(string.format("[countin] wrote main RAM to %s", path))
    end,
})
