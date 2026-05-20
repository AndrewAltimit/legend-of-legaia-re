-- autorun_man_source.lua
--
-- Pins where a field scene's runtime MAN buffer (_DAT_8007b898) comes from.
--
-- The MAN buffer is malloc'd + filled by the asset-type dispatcher
-- FUN_8001F05C (case 3): `param_1`(a0) = source bytes, `param_2`(a1) =
-- (type<<24)|size, and case 3 does `_DAT_8007b898 = malloc(size)` then
-- LZS-decodes/copies the source into it. So an Exec breakpoint at the
-- dispatcher entry, filtered to a1>>24 == 3, catches every MAN load with
-- its source pointer + size + caller RA.
--
-- This probe:
--   1. dumps the RESIDENT MAN (`_DAT_8007b898`) at capture start - the
--      ground-truth bytes for the scene the save is parked in;
--   2. arms the dispatcher Exec BP; on each MAN dispatch it logs
--      a0/size/a2/a3/ra, captures call context, and dumps the source
--      bytes - so a driven transition reveals the loader + container.
--
-- Run (drive a transition with LEGAIA_HOLD_BUTTON / LEGAIA_HOLD):
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate1 \
--   LEGAIA_HOLD_BUTTON=4 LEGAIA_HOLD=400 LEGAIA_FRAMES=900 \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_man_source.lua \
--       bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 900)
local HOLD_BUTTON = probe.getenv_num("LEGAIA_HOLD_BUTTON", 0)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD", 0)
local OUT_PATH    = probe.out_path("man_source.csv")
local DETAIL_PATH = OUT_PATH:gsub("%.csv$", ".detail.txt")
local DUMP_DIR    = OUT_PATH:gsub("man_source%.csv$", "")

local FUN_8001F05C = 0x8001F05C
local DAT_b898     = 0x8007B898
local DUMP_LEN     = 0x6000

local csv = probe.csv_open(OUT_PATH, "tick,type,a0_src,size,a2,a3,ra,man_buf")

local function dump_bytes(path, addr, len)
    if not probe.in_ram(addr, 1) then return false end
    local fh = io.open(path, "wb")
    if not fh then return false end
    local off = 0
    while off < len do
        local n = math.min(0x4000, len - off)
        local b = probe.read_bytes(addr + off, n)
        if b == nil then break end
        fh:write(tostring(b))
        off = off + n
    end
    fh:close()
    return true
end

local dumped_resident = false
local man_hits = 0

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),
    hold_button    = HOLD_BUTTON ~= 0 and HOLD_BUTTON or nil,
    hold_frames    = HOLD_FRAMES,

    on_arm = function()
        local d = { addr = FUN_8001F05C, name = "asset_dispatch", hits_ref = { n = 0 } }
        probe.arm_breakpoint(FUN_8001F05C, "Exec", 4, d.name, function()
            local r  = PCSX.getRegisters()
            local a1 = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
            local atype = bit.band(bit.rshift(a1, 24), 0xFF)
            d.hits_ref.n = d.hits_ref.n + 1
            if atype ~= 3 then return end           -- only MAN dispatches
            man_hits = man_hits + 1
            local a0 = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
            local a2 = bit.band(tonumber(r.GPR.n.a2) or 0, 0xFFFFFFFF)
            local a3 = bit.band(tonumber(r.GPR.n.a3) or 0, 0xFFFFFFFF)
            local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            local size = bit.band(a1, 0xFFFFFF)
            local man_buf = probe.read_u32(DAT_b898)
            local scene = tostring(probe.read_bytes(0x80084548, 12) or ""):gsub("%z.*$", "")
            csv:row("%d,%d,0x%08X,0x%06X,0x%08X,0x%08X,0x%08X,0x%08X",
                man_hits, atype, a0, size, a2, a3, ra, man_buf)
            PCSX.log(string.format(
                "[man] MAN dispatch #%d scene='%s': src=0x%08X size=0x%X ra=0x%08X a2=0x%X a3=0x%X",
                man_hits, scene, a0, size, ra, a2, a3))
            local snap = probe.capture_call_context(string.format(
                "MAN dispatch #%d src=0x%08X size=0x%X ra=0x%08X", man_hits, a0, size, ra))
            probe.append_call_context(DETAIL_PATH, snap)
            -- Dump the source bytes (the dispatcher's input - the disc/RAM
            -- buffer the MAN is decoded from).
            dump_bytes(string.format("%sman_src_%02d.bin", DUMP_DIR, man_hits),
                a0, math.min(size + 0x40, DUMP_LEN))
        end)
        return { d }
    end,

    on_capture = function(_ctx, elapsed)
        if dumped_resident or elapsed < 2 then return end
        dumped_resident = true
        local man = probe.read_u32(DAT_b898)
        local mode = probe.read_u8(0x8007B83C)
        PCSX.log(string.format(
            "[man] resident scene: game_mode=0x%02X _DAT_8007b898=0x%08X", mode, man))
        if man ~= 0 then
            dump_bytes(DUMP_DIR .. "man_resident.bin", man, DUMP_LEN)
            PCSX.log("[man] resident MAN dumped to man_resident.bin")
        end
    end,

    on_done = function()
        csv:close()
        PCSX.log(string.format("=== MAN source probe: %d MAN dispatch(es) caught ===", man_hits))
    end,
})
