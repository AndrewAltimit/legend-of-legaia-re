-- autorun_super_art_queue_builder.lua
--
-- Pins the runtime queue-builder that emits a Super / Miracle Art's
-- interleaved action queue (the combo-specific *connector* bytes).
--
-- Background: the Super/Miracle MATCHER is ported + wired (which combo
-- triggers which Super), but the byte-exact queue is not. The connector
-- after each component art is combo-specific -- Vahn's `0x27` is followed by
-- `0F` in Tri-Somersault but `0E` in Power Slash -- so it cannot be derived
-- from each art's own command string. The runtime builder writes the queue
-- into the battle-action context at `ctx[+0x274]` (the "queued action" byte
-- the action SM state 0x00 copies into `actor[+0x1A]`; `ctx[+0x276]` is the
-- "queued from menu" flag). The builder's PC + the literal connector bytes
-- have never been captured. See docs/subsystems/battle-action.md
-- ("Miracle / Super in the live player-driven Arts submenu") + the F-SUPER /
-- D-ARTS threads in docs/reference/open-rev-eng-threads.md.
--
-- `ctx` is the pointer global `_DAT_8007BD24` (a `byte *`; the action SM uses
-- `pbVar = _DAT_8007BD24; pbVar[0x274]`). So the queue field is
-- `read_u32(0x8007BD24) + 0x274`. We read the pointer at arm time (it is live
-- in any battle save) and watch a small window over `+0x274..+0x278` for
-- writes, logging the writer PC + value + GPRs and snapshotting the
-- `+0x270..+0x2A0` queue region on every hit so the queue's growth is visible.
--
-- HOW TO CAPTURE (PCSX-Redux save state required -- mednafen cannot drive a
-- live watchpoint): see docs/tooling/super-art-queue-capture.md. In short,
-- save the instant a Super/Miracle Art combo is COMMITTED and resolving (the
-- chained arts are on screen about to execute), then:
--   LEGAIA_FRAMES=1200 \
--   timeout --kill-after=20s 600s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --sstate <your-super-combo.sstate> \
--       --lua scripts/pcsx-redux/autorun_super_art_queue_builder.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1200)
local OUT_PATH = probe.out_path("super_art_queue_builder.csv")

local CTX_PTR  = 0x8007BD24 -- pointer global; *(CTX_PTR) = ctx base
local Q_OFF    = 0x274      -- queued-action byte (ctx[+0x274])
local FLAG_OFF = 0x276      -- "queued from menu" flag (ctx[+0x276])
-- Snapshot window so the whole queue region is visible on each write.
local SNAP_LO  = 0x270
local SNAP_HI  = 0x2A0

local GPR_NAMES = {
    "at", "v0", "v1", "a0", "a1", "a2", "a3",
    "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7",
    "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "t8", "t9", "k0", "k1", "gp", "sp", "s8", "ra",
}
local function gpr_dump(r)
    local p = {}
    for _, nm in ipairs(GPR_NAMES) do
        local ok, v = pcall(function() return tonumber(r.GPR.n[nm]) % 0x100000000 end)
        if ok then p[#p + 1] = string.format("%s=%08X", nm, v) end
    end
    return table.concat(p, " ")
end

local function ctx_base()
    local p = probe.read_u32(CTX_PTR) or 0
    return p % 0x100000000
end

local csv = probe.csv_open(OUT_PATH, "tick,pc,addr,off,width,newval,queue_hex")
local logged = 0
local LOG_CAP = 200

local function snapshot_queue(base)
    local b = probe.read_bytes(base + SNAP_LO, SNAP_HI - SNAP_LO)
    if b == nil then return "" end
    return probe.bytes_to_hex(b):gsub("%s+", "")
end

local function on_write(off, width)
    return function()
        local r = PCSX.getRegisters()
        local pc = (tonumber(r.pc) or 0) % 0x100000000
        if logged >= LOG_CAP then return end
        logged = logged + 1
        local base = ctx_base()
        local newval
        if width == 1 then
            newval = probe.read_u8(base + off) or 0
        else
            newval = probe.read_u16(base + off) or 0
        end
        local qhex = snapshot_queue(base)
        csv:row("%d,0x%08X,0x%08X,0x%X,%d,0x%X,%s",
            logged, pc, base + off, off, width, newval, qhex)
        PCSX.log(string.format(
            "[superq] #%d pc=0x%08X ctx+0x%X=0x%X (ctx=0x%08X)",
            logged, pc, off, newval, base))
        PCSX.log(string.format("[superq]   queue[+0x%X..+0x%X]=%s",
            SNAP_LO, SNAP_HI, qhex))
        PCSX.log(string.format("[superq]   GPR %s", gpr_dump(r)))
    end
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        local base = ctx_base()
        if not probe.in_ram(base) then
            PCSX.log(string.format(
                "[superq] WARNING: ctx pointer *(0x%08X)=0x%08X is not in RAM -- "
                .. "is this a battle save? Arming anyway from the current value.",
                CTX_PTR, base))
        end
        PCSX.log(string.format("[superq] ctx base = 0x%08X; watching +0x%X (w1) and +0x%X (w1)",
            base, Q_OFF, FLAG_OFF))
        probe.arm_breakpoint(base + Q_OFF, "Write", 1, "q274", on_write(Q_OFF, 1))
        probe.arm_breakpoint(base + FLAG_OFF, "Write", 1, "q276", on_write(FLAG_OFF, 1))
        return {
            { addr = base + Q_OFF, name = "q274" },
            { addr = base + FLAG_OFF, name = "q276" },
        }
    end,

    on_done = function()
        csv:close()
        PCSX.log(string.format(
            "=== super-art queue-builder probe: writes logged=%d (cap %d) ctx=0x%08X ===",
            logged, LOG_CAP, ctx_base()))
        if logged == 0 then
            PCSX.log("[superq] NO writes to ctx+0x274/+0x276 fired. Either the save "
                .. "was not at a Super/Miracle combo commit, or ctx was not yet "
                .. "allocated. Save the frame the combo resolves and re-run.")
        end
    end,
})
