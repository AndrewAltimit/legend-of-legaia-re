-- autorun_world_map_probe.lua
--
-- Closed-loop world-map VM probe. Loads a save state, arms exec
-- breakpoints across 8 world-map subsystem entry points, captures
-- per-call samples at the draw-VM dispatcher (FUN_801D362C) and OT
-- cursor reads at FUN_801D7EA0, and writes two CSVs + a live hit
-- snapshot.
--
-- Env vars:
--   LEGAIA_SSTATE        save state path (default: sstate1)
--   LEGAIA_FRAMES        post-load capture vsyncs (default 600)
--   LEGAIA_OUT           main CSV path (default world_map_probe.csv;
--                        OT cursor CSV is derived as <stem>.ot_cursor.csv,
--                        live snapshot as <stem>.hits.txt)
--   LEGAIA_HOLD_UP       D-pad UP hold duration in vsyncs (default 0)
--
-- Outputs:
--   <OUT>                  draw-VM samples (call_idx, a0, a1, sub_op, bytes_hex)
--   <stem>.ot_cursor.csv   OT-cursor reads (hit_idx, cursor, a0, a1)
--   <stem>.hits.txt        live hit counts, rewritten every 60 vsyncs

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 600)
local OUT_PATH    = probe.out_path("world_map_probe.csv")
local HOLD_UP     = probe.getenv_num("LEGAIA_HOLD_UP", 0)

local OT_PATH   = OUT_PATH:gsub("%.csv$", ".ot_cursor.csv")
local SNAP_PATH = OUT_PATH:gsub("%.csv$", ".hits.txt")

-- Probe target addresses + names. The draw-VM dispatcher (0x801D362C) and
-- the OT-pool emitter (0x801D7EA0) get rich per-call captures; the rest
-- just tally hit counts.
local SAMPLE_PROBE_ADDR = 0x801D362C
local OT_PROBE_ADDR     = 0x801D7EA0
local OT_CURSOR_ADDR    = 0x1F8003A0  -- scratchpad: current OT prim ptr
local SAMPLE_DUMP_LEN   = 64

local PROBE_ADDRS = {
    { addr = 0x80017EC8, name = "AddPrim_dispatch_sanity" },
    { addr = 0x801E76D4, name = "world_map_controller" },
    { addr = 0x80023070, name = "move_vm_entry" },
    { addr = 0x80023AE0, name = "move_vm_op_0x2F" },
    { addr = SAMPLE_PROBE_ADDR, name = "world_map_draw_vm" },
    { addr = 0x801D31B0, name = "scanline_emitter" },
    { addr = 0x801D6704, name = "world_map_main_init" },
    { addr = OT_PROBE_ADDR, name = "FUN_801D7EA0_POLY_FT4_emit" },
}

PCSX.log(string.format("[wm] sstate=%s frames=%d out=%s ot=%s",
    SSTATE_PATH, FRAMES, OUT_PATH, OT_PATH))

local csv = probe.csv_open(OUT_PATH,
    "call_idx,a0_render_ctx,a1_bytecode_pc,sub_op,bytes_hex")
local ot_csv = probe.csv_open(OT_PATH,
    "hit_idx,ot_cursor_at_entry,a0,a1")

local sample_idx = 0
local ot_hit_idx = 0

local function n32(v) return bit.band(v, 0xFFFFFFFF) end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    snapshot_path  = SNAP_PATH,
    snapshot_every = 60,
    hold_button    = (HOLD_UP > 0) and probe.BTN.UP or nil,
    hold_frames    = HOLD_UP,

    on_arm = function(_)
        local descs = {}
        for _, pa in ipairs(PROBE_ADDRS) do
            local addr, name = pa.addr, pa.name
            local d = { addr = addr, name = name, hits_ref = { n = 0 } }
            local cb
            if addr == SAMPLE_PROBE_ADDR then
                cb = function()
                    d.hits_ref.n = d.hits_ref.n + 1
                    local r = PCSX.getRegisters()
                    local a0 = n32(tonumber(r.GPR.n.a0) or 0)
                    local a1 = n32(tonumber(r.GPR.n.a1) or 0)
                    sample_idx = sample_idx + 1
                    local sub_op = probe.read_u16(a1 + 2) or 0xFFFF
                    local raw = probe.read_bytes(a1, SAMPLE_DUMP_LEN)
                    local hex = raw and probe.bytes_to_hex(raw) or ""
                    csv:row("%d,0x%08X,0x%08X,0x%04X,%s",
                        sample_idx - 1, a0, a1, sub_op, hex)
                    if d.hits_ref.n <= 8 then
                        PCSX.log(string.format(
                            "[wm] draw_vm hit %d: a0=0x%08X a1=0x%08X sub_op=0x%04X",
                            d.hits_ref.n, a0, a1, sub_op))
                    end
                end
            elseif addr == OT_PROBE_ADDR then
                cb = function()
                    d.hits_ref.n = d.hits_ref.n + 1
                    ot_hit_idx = ot_hit_idx + 1
                    local r = PCSX.getRegisters()
                    local a0 = n32(tonumber(r.GPR.n.a0) or 0)
                    local a1 = n32(tonumber(r.GPR.n.a1) or 0)
                    local cursor = n32(probe.read_scratch_u32(OT_CURSOR_ADDR))
                    ot_csv:row("%d,0x%08X,0x%08X,0x%08X",
                        ot_hit_idx, cursor, a0, a1)
                    if ot_hit_idx <= 3 then
                        PCSX.log(string.format(
                            "[wm] FUN_801D7EA0 hit %d: OT=0x%08X a0=0x%08X a1=0x%08X",
                            ot_hit_idx, cursor, a0, a1))
                    end
                end
            else
                cb = function() d.hits_ref.n = d.hits_ref.n + 1 end
            end
            probe.arm_breakpoint(addr, "Exec", 4, name, cb)
            descs[#descs + 1] = d
        end
        return descs
    end,

    on_done = function(_, descs)
        csv:close()
        ot_csv:close()
        PCSX.log(string.format("[wm] %d sample rows in %s", sample_idx, OUT_PATH))
        PCSX.log(string.format("[wm] %d OT-cursor rows in %s", ot_hit_idx, OT_PATH))
        PCSX.log("=== world-map probe hits ===")
        for _, d in ipairs(descs) do
            PCSX.log(string.format("  0x%08X  %8d  %s",
                d.addr, d.hits_ref.n, d.name))
        end
        PCSX.log("=== end ===")
    end,
})
