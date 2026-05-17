-- autorun_countdown_trigger.lua
--
-- Memory-watchpoint-driven RAM + screenshot snapshot. Arms a Write BP
-- on _DAT_801EF16C (the title-attract countdown register) and, on the
-- (HIT_SKIP+1)-th hit, snapshots registers + framebuffer + a deferred
-- chunked RAM window dump.
--
-- The countdown gets written twice during boot:
--   1. CD-DMA-channel-3 read at FUN_8005D9A0 stamps the 0x8000 sentinel
--      into the title-overlay BSS region — fires early, before the
--      title overlay's tick function ever runs.
--   2. The title overlay's per-frame tick decrements it once the title
--      screen has become live. This is the moment we want to capture —
--      PC + RA pin the title-overlay tick function.
--
-- LEGAIA_HIT_SKIP controls how many BP hits to ignore before capturing:
--   * 1 (default for save state taken pre-boot): skip bulk init, snap on
--     the first decrement.
--   * 0 (for a save state taken AT the title screen): bulk init has
--     already fired, the FIRST BP hit is a decrement.
--
-- Env vars:
--   LEGAIA_SSTATE        path to .sstate (default sstate7)
--   LEGAIA_OUT           output .bin path (default countdown_trigger.bin)
--   LEGAIA_WATCH_ADDR    hex address to watch (default 0x801EF16C)
--   LEGAIA_HIT_SKIP      BP hits to ignore (default 1)
--   LEGAIA_MAX_WAIT      max vsyncs to wait for the BP (default 600)
--   LEGAIA_DUMP_BASE     RAM dump base address (default 0x801C0000)
--   LEGAIA_DUMP_LEN      RAM dump length (default 0x40000 = overlay window)
--
-- Outputs:
--   <OUT>                 RAM window dump
--   <OUT>.regs            text register snapshot
--   <OUT>.screen          raw framebuffer pixels (BGR555 or BGR888)
--   <OUT>.screen.meta     width=N\nheight=N\nbpp={16,24}\n

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7")
local OUT_PATH    = probe.out_path("countdown_trigger.bin")
local MAX_WAIT    = probe.getenv_num("LEGAIA_MAX_WAIT", 600)
local HIT_SKIP    = probe.getenv_num("LEGAIA_HIT_SKIP", 1)
local DUMP_BASE   = probe.getenv_num("LEGAIA_DUMP_BASE", 0x801C0000)
local DUMP_LEN    = probe.getenv_num("LEGAIA_DUMP_LEN",  0x40000)

-- WATCH_ADDR: parse env in either hex (0x...) or decimal form.
local WATCH_ADDR = 0x801EF16C
local watch_env = os.getenv("LEGAIA_WATCH_ADDR")
if watch_env and watch_env ~= "" then
    if watch_env:match("^0[xX]") then
        WATCH_ADDR = tonumber(watch_env, 16)
    else
        WATCH_ADDR = tonumber(watch_env)
    end
end
if WATCH_ADDR == nil then WATCH_ADDR = 0x801EF16C end

local CHUNK = 0x40000  -- one shot per vsync (matches default DUMP_LEN)

PCSX.log(string.format(
    "[trigger] dump base=0x%08X len=0x%X chunk=0x%X",
    DUMP_BASE, DUMP_LEN, CHUNK))
PCSX.log(string.format(
    "[trigger] sstate=%s out=%s watch=0x%08X hit_skip=%d max_wait=%d",
    SSTATE_PATH, OUT_PATH, WATCH_ADDR, HIT_SKIP, MAX_WAIT))

-- Sub-state machine inside on_capture.
local S_WAITING = 1
local S_DUMPING = 2
local S_QUIT    = 3

local sub_state = S_WAITING
local hit_count = 0
local dump_fh   = nil
local dump_off  = 0

local function n32(v) return bit.band(v, 0xFFFFFFFF) end

local function begin_dump(reason)
    local r = PCSX.getRegisters()
    local pc = n32(tonumber(r.pc) or 0)
    local ra = n32(tonumber(r.GPR.n.ra) or 0)

    -- Register sidecar.
    local rfh = io.open(OUT_PATH .. ".regs", "w")
    if rfh ~= nil then
        rfh:write(string.format("# countdown_trigger snapshot (%s)\n", reason))
        rfh:write(string.format("watch_addr  0x%08X\n", WATCH_ADDR))
        rfh:write(string.format("hit_count   %d\n", hit_count))
        rfh:write(string.format("hit_skip    %d\n", HIT_SKIP))
        rfh:write(string.format("pc  0x%08X\n", pc))
        rfh:write(string.format("gp  0x%08X\n", n32(tonumber(r.GPR.n.gp) or 0)))
        rfh:write(string.format("sp  0x%08X\n", n32(tonumber(r.GPR.n.sp) or 0)))
        rfh:write(string.format("ra  0x%08X\n", ra))
        rfh:write(string.format("a0  0x%08X\n", n32(tonumber(r.GPR.n.a0) or 0)))
        rfh:write(string.format("a1  0x%08X\n", n32(tonumber(r.GPR.n.a1) or 0)))
        rfh:write(string.format("a2  0x%08X\n", n32(tonumber(r.GPR.n.a2) or 0)))
        rfh:write(string.format("a3  0x%08X\n", n32(tonumber(r.GPR.n.a3) or 0)))
        rfh:write(string.format("s8  0x%08X\n", n32(tonumber(r.GPR.n.s8) or 0)))
        rfh:close()
    end
    PCSX.log(string.format(
        "[trigger] capture (%s): pc=0x%08X ra=0x%08X", reason, pc, ra))

    -- Screenshot before the chunked RAM dump (survives if dump aborts).
    local ss_ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if ss_ok and ss ~= nil and ss.data ~= nil then
        local ss_path = OUT_PATH .. ".screen"
        local sfh = io.open(ss_path, "wb")
        if sfh ~= nil then
            sfh:write(tostring(ss.data))
            sfh:close()
            local bpp_str = (ss.bpp == 0) and "16" or "24"
            PCSX.log(string.format(
                "[trigger] wrote screenshot %dx%d bpp=%s to %s",
                tonumber(ss.width), tonumber(ss.height), bpp_str, ss_path))
            local mfh = io.open(ss_path .. ".meta", "w")
            if mfh ~= nil then
                mfh:write(string.format("width=%d\n", tonumber(ss.width)))
                mfh:write(string.format("height=%d\n", tonumber(ss.height)))
                mfh:write(string.format("bpp=%s\n", bpp_str))
                mfh:close()
            end
        end
    else
        PCSX.log(string.format(
            "[trigger] WARN: takeScreenShot failed (%s)", tostring(ss)))
    end

    dump_fh = io.open(OUT_PATH, "wb")
    if dump_fh == nil then
        PCSX.log(string.format("[trigger] FATAL: cannot open %s", OUT_PATH))
        return false
    end
    dump_off = 0
    return true
end

-- Pull one CHUNK from emulator RAM, write to disk. Returns true when
-- the dump is complete.
local function advance_dump()
    if dump_fh == nil then return true end
    local remaining = DUMP_LEN - dump_off
    if remaining <= 0 then
        dump_fh:close(); dump_fh = nil
        return true
    end
    local n = math.min(CHUNK, remaining)
    local buf = probe.read_bytes(DUMP_BASE + dump_off, n)
    if buf == nil then
        PCSX.log(string.format(
            "[trigger] FATAL: read failed at off=0x%X", dump_off))
        dump_fh:close(); dump_fh = nil
        return true
    end
    dump_fh:write(tostring(buf))
    dump_off = dump_off + n
    if dump_off >= DUMP_LEN then
        dump_fh:close(); dump_fh = nil
        PCSX.log(string.format(
            "[trigger] wrote %d bytes (base=0x%08X) to %s",
            DUMP_LEN, DUMP_BASE, OUT_PATH))
        return true
    end
    return false
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = MAX_WAIT,

    on_arm = function(_)
        local d = {
            addr = WATCH_ADDR,
            name = "trigger:countdown",
            hits_ref = { n = 0 },
        }
        probe.arm_breakpoint(WATCH_ADDR, "Write", 2, "trigger:countdown",
            function()
                if sub_state ~= S_WAITING then return end
                hit_count = hit_count + 1
                d.hits_ref.n = hit_count
                if hit_count <= HIT_SKIP then
                    local r = PCSX.getRegisters()
                    PCSX.log(string.format(
                        "[trigger] skipping hit %d/%d (pc=0x%08X ra=0x%08X)",
                        hit_count, HIT_SKIP,
                        n32(tonumber(r.pc) or 0),
                        n32(tonumber(r.GPR.n.ra) or 0)))
                    return
                end
                -- Snapshot regs + screen now (inside the BP callback so
                -- pc/ra reflect the exact write instruction); defer the
                -- heavy RAM read to vsync.
                if begin_dump(string.format("watchpoint:hit#%d", hit_count)) then
                    sub_state = S_DUMPING
                else
                    sub_state = S_QUIT
                end
            end)
        PCSX.log(string.format(
            "[trigger] watchpoint armed at 0x%08X (width=2, kind=Write, skip=%d)",
            WATCH_ADDR, HIT_SKIP))
        return { d }
    end,

    on_capture = function(ctx, elapsed)
        if sub_state == S_WAITING then
            -- Heartbeat: log live countdown value every 60 vsyncs.
            if elapsed > 0 and elapsed % 60 == 0 then
                local cur = probe.read_u16(WATCH_ADDR)
                PCSX.log(string.format(
                    "[trigger] heartbeat rel=%d  hits=%d  cur=%s",
                    elapsed, hit_count,
                    cur and string.format("0x%04X", cur) or "?"))
            end
        elseif sub_state == S_DUMPING then
            if advance_dump() then
                sub_state = S_QUIT
                ctx.request_quit = true
            end
        end
    end,
})
