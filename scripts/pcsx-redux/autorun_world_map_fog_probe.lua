-- autorun_world_map_fog_probe.lua
--
-- Probes the five distance-cue fog parameters consulted by the world-map
-- overlay's per-prim leaves at 0x801F7644..0x801F8690 (see
-- docs/subsystems/world-map.md and crates/engine-vm/src/prim_dispatch.rs).
--
-- Each leaf inserts a ~60-instruction fog block between GTE projection and
-- the OT packet write, reading these GP-relative fields every vertex:
--
--   gp-0x2E0  u32   Far-plane reference Z (mixed into prim cmd word).
--   gp-0x2DC  u32   Fog color (loaded into GTE color register pre dpcs).
--   gp-0x2D1  u8    Fog-enable flags byte; bit 0x10 gates the whole path.
--   gp-0x2BC  u32   Pointer to per-Z fog-tint LUT (2-byte entries, Z>>5).
--   gp+0x90   u8    Z shift exponent (Z_far = max(z1..) >> *(u8 *)).
--
-- Reads gp from the live register file after save-state load, computes
-- absolute addresses, snapshots initial values, then arms width-matched
-- Read breakpoints. The top PCs surface which overlay leaves are firing on
-- the current frame; cross-reference against the leaf table in
-- docs/subsystems/world-map.md to pin which slot (12..19) is producing
-- each prim. The LUT at gp-0x2BC is dumped to a sidecar `.lut.bin` on
-- every snapshot tick so the WebGL port can bake the equivalent table.
--
-- Env vars:
--   LEGAIA_SSTATE        save state in world-map top-view dev menu (default sstate1)
--   LEGAIA_FRAMES        post-load vsyncs (default 600)
--   LEGAIA_OUT           CSV path (default fog_probe.csv)
--   LEGAIA_GP            gp override (decimal/0x-hex); 0 = use live register
--   LEGAIA_HOLD_BUTTON   PSX pad bit to hold (0 = no hold)
--   LEGAIA_HOLD          hold duration in vsyncs (default 0)
--
-- Outputs:
--   <OUT>              per-hit (probe_idx, addr, pc, width, value, ra)
--   <stem>.snap.txt    per-frame snapshot of fog field values + LUT addr
--   <stem>.lut.bin     1 KiB raw fog LUT (512 u16 entries, Z>>5)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 600)
local OUT_PATH    = probe.out_path("fog_probe.csv")
local GP_OVERRIDE = probe.getenv_num("LEGAIA_GP", 0)
local HOLD_BUTTON = probe.getenv_num("LEGAIA_HOLD_BUTTON", 0)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD", 0)

local SNAP_PATH = OUT_PATH:gsub("%.csv$", ".snap.txt")
local LUT_PATH  = OUT_PATH:gsub("%.csv$", ".lut.bin")

-- (offset_from_gp, width_bytes, label). Order = probe_idx column.
local FOG_FIELDS = {
    { off = -0x2E0, width = 4, label = "far_ref" },
    { off = -0x2DC, width = 4, label = "fog_color" },
    { off = -0x2D1, width = 1, label = "enable" },
    { off = -0x2BC, width = 4, label = "lut_ptr" },
    { off =  0x90,  width = 1, label = "z_shift" },
}
local LUT_DUMP_BYTES   = 1024
local MAX_HITS_PER     = 1000

PCSX.log(string.format(
    "[fog] sstate=%s frames=%d out=%s snap=%s lut=%s",
    SSTATE_PATH, FRAMES, OUT_PATH, SNAP_PATH, LUT_PATH))

local csv = probe.csv_open(OUT_PATH,
    "probe_idx,addr,pc,width,value,ra")

local function n32(v) return bit.band(v, 0xFFFFFFFF) end
local function fmt_addr(v) return string.format("0x%08X", n32(v)) end

local function read_field(addr, width)
    if width == 1 then return probe.read_u8(addr) end
    return probe.read_u32(addr)
end

local function dump_lut(addr, bytes)
    return probe.read_bytes(addr, bytes)
end

local function write_snapshot(label, vsync, addrs, hits, gp_base)
    local f = io.open(SNAP_PATH, "w")
    if not f then return end
    f:write(string.format(
        "# %s  vsync=%d  gp=%s\n", label, vsync, fmt_addr(gp_base)))
    for i, field in ipairs(FOG_FIELDS) do
        local addr = addrs[i] or 0
        local v = read_field(addr, field.width)
        local hit = hits[i] or 0
        local capped = hit > MAX_HITS_PER and " (capped)" or ""
        f:write(string.format(
            "  probe %d  %-9s  %s  width=%d  val=%s  hits=%d%s\n",
            i - 1, field.label, fmt_addr(addr), field.width,
            v and string.format("0x%08X", v) or "<oob>", hit, capped))
    end
    local lut_ptr = probe.read_u32(addrs[4] or 0)
    if lut_ptr and probe.in_ram(lut_ptr, LUT_DUMP_BYTES) then
        local blob = dump_lut(lut_ptr, LUT_DUMP_BYTES)
        if blob then
            local lf = io.open(LUT_PATH, "wb")
            if lf then lf:write(tostring(blob)); lf:close() end
            f:write(string.format(
                "  lut: %s (%d bytes) written to %s\n",
                fmt_addr(lut_ptr), LUT_DUMP_BYTES, LUT_PATH))
        end
    else
        f:write(string.format(
            "  lut: ptr=%s out-of-range; LUT not yet populated\n",
            lut_ptr and fmt_addr(lut_ptr) or "<nil>"))
    end
    f:close()
end

local probe_addrs = {}  -- computed after gp is known
local hits        = {}
local gp_base     = 0

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    snapshot_path  = nil,  -- we manage our own snapshot at SNAP_PATH
    hold_button    = (HOLD_BUTTON ~= 0 and HOLD_FRAMES > 0) and HOLD_BUTTON or nil,
    hold_frames    = HOLD_FRAMES,

    on_arm = function(_)
        -- gp is restored by the save-state load that probe.run just did.
        local r = PCSX.getRegisters()
        gp_base = GP_OVERRIDE ~= 0 and GP_OVERRIDE or n32(tonumber(r.GPR.n.gp) or 0)
        if gp_base == 0 then
            PCSX.log("[fog] FATAL: gp=0 after load and no LEGAIA_GP override")
            PCSX.quit(3)
            return {}
        end

        local descs = {}
        for i, field in ipairs(FOG_FIELDS) do
            local idx   = i
            local addr  = n32(gp_base + field.off)
            local width = field.width
            local label = field.label
            probe_addrs[i] = addr
            hits[i] = 0
            local d = {
                addr = addr,
                name = string.format("fog:%s", label),
                hits_ref = { n = 0 },
            }
            probe.arm_breakpoint(addr, "Read", width, label, function()
                hits[idx] = hits[idx] + 1
                d.hits_ref.n = hits[idx]
                if hits[idx] > MAX_HITS_PER then return end
                local rr = PCSX.getRegisters()
                local pc = n32(tonumber(rr.pc) or 0)
                local ra = n32(tonumber(rr.GPR.n.ra) or 0)
                local v  = read_field(addr, width) or 0
                csv:row("%d,%s,%s,%d,0x%08X,%s",
                    idx - 1, fmt_addr(addr), fmt_addr(pc), width, v, fmt_addr(ra))
                if hits[idx] <= 3 then
                    PCSX.log(string.format(
                        "[fog] probe %d %s (%s) hit %d: pc=%s val=0x%08X ra=%s",
                        idx - 1, label, fmt_addr(addr), hits[idx],
                        fmt_addr(pc), v, fmt_addr(ra)))
                end
                if hits[idx] == MAX_HITS_PER then
                    PCSX.log(string.format(
                        "[fog] probe %d %s cap reached at %d hits",
                        idx - 1, label, MAX_HITS_PER))
                end
            end)
            descs[#descs + 1] = d
        end
        PCSX.log(string.format("[fog] %d Read probes armed (gp=%s)",
            #FOG_FIELDS, fmt_addr(gp_base)))
        write_snapshot("initial", 0, probe_addrs, hits, gp_base)
        return descs
    end,

    on_capture = function(_, elapsed)
        if elapsed > 0 and elapsed % 60 == 0 then
            write_snapshot("live", elapsed, probe_addrs, hits, gp_base)
        end
    end,

    on_done = function(_, _)
        write_snapshot("final", FRAMES, probe_addrs, hits, gp_base)
        csv:close()
        PCSX.log("=== fog probe hit counts ===")
        for i, field in ipairs(FOG_FIELDS) do
            local capped = (hits[i] or 0) > MAX_HITS_PER and " (capped)" or ""
            PCSX.log(string.format(
                "  probe %d  %-9s  %s  hits=%d%s",
                i - 1, field.label, fmt_addr(probe_addrs[i] or 0),
                hits[i] or 0, capped))
        end
        PCSX.log("=== end ===")
    end,
})
