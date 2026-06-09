-- autorun_slot4_source_map.lua
--
-- Pins the world-map kingdom slot-4 RECORD -> working-buffer transcode by
-- correlating each READ of the slot-4 RAM window with the DESTINATION it is
-- copied to during the kingdom-warp scene load. This is the documented
-- "finer probe" that closes the F-WM thread (docs/formats/world-map-overlay.md
-- "Working-buffer writers"): the earlier hunt established that FUN_8001E54C (the
-- [type, size, data] streaming-chunk processor, memcpy inner loop 0x8001A8C8)
-- copies slot-4 chunks somewhere, and that slot-4 RAM is touched once during the
-- warp -- but never pinned WHICH chunks come from slot 4 and WHERE they land.
--
-- Strategy:
--   1. Read bps tiled across the Drake slot-4 window (0x8011A624 + k*0x800).
--      Each fires when the scene-load copy crosses that offset; the callback
--      records the faulting source offset plus the full GPR set, so post-
--      analysis can identify the destination register (the one whose value
--      tracks faulting_src - slot4_base + dst_base) without knowing the memcpy's
--      register convention a priori.
--   2. An Exec bp at FUN_8001E54C entry logs every streaming-chunk dispatch
--      during the warp: ra (which feeder), a0..a3, and the chunk header words at
--      a0 (type byte at +3, size). Cross-referencing a dispatch whose chunk data
--      lies inside the slot-4 window with the read-bp dst gives the per-chunk
--      (slot4 source -> working-buffer destination) map.
--
-- Run (the existing pre-warp Drake save drives the transition itself):
--   LEGAIA_FRAMES=600 LEGAIA_HOLD_BUTTON=16 LEGAIA_HOLD=90 \
--   LEGAIA_OUT=captures/slot4_source_map/drake.csv \
--     timeout --kill-after=30s 600s bash scripts/pcsx-redux/run_probe.sh \
--       --scenario drake_castle_to_worldmap \
--       --lua scripts/pcsx-redux/autorun_slot4_source_map.lua
--
-- LEGAIA_HOLD_BUTTON is the PSX pad mask for the exit direction (Drake's south
-- exit warp = Up = 0x10; pass 0x40 for Left, 0x80 for Right, 0x40<<... per pad).
-- The Drake save's documented trigger is a held Up press, so 0x10.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 600)
local OUT_PATH    = probe.out_path("slot4_source_map.csv")
local HOLD_BUTTON = probe.getenv_num("LEGAIA_HOLD_BUTTON", 0)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD", 0)

-- Drake kingdom-bundle slot-4 RAM window (docs/formats/world-map-overlay.md):
-- payload at 0x8011A624, 32304 = 0x7E30 bytes, 15 sub-bodies.
local SLOT4_BASE = probe.getenv_num("LEGAIA_SLOT4_BASE", 0x8011A624)
local SLOT4_SIZE = probe.getenv_num("LEGAIA_SLOT4_SIZE", 0x7E30)
local STRIDE     = probe.getenv_num("LEGAIA_SLOT4_STRIDE", 0x800)
local MAX_HITS   = probe.getenv_num("LEGAIA_RD_CAP", 40)

-- Streaming-chunk dispatcher (the scene-load chunk loader).
local FUN_8001E54C = 0x8001E54C

local DETAIL_PATH = OUT_PATH:gsub("%.csv$", ".detail.txt")

local csv = probe.csv_open(OUT_PATH,
    "kind,src,pc,ra,a0,a1,a2,a3,v0,v1,t0,t1,t2,t3,t4,t5,t6,t7,s0,s1,s2,s3")

local function read_u32(addr)
    local mem = PCSX.getMemoryAsFile()
    local buf = mem:readAt(4, bit.band(addr, 0x1FFFFFFF))
    if buf == nil then return 0 end
    local s = tostring(buf)
    return string.byte(s, 1)
        + string.byte(s, 2) * 0x100
        + string.byte(s, 3) * 0x10000
        + string.byte(s, 4) * 0x1000000
end

local function reg(r, name)
    return tonumber(r.GPR.n[name]) or 0
end

-- Emit one CSV row from the live register file. `kind` is "rd" for a slot-4
-- read or "disp" for a FUN_8001E54C dispatch; `src` is the faulting slot-4
-- address (reads) or 0 (dispatch).
local function emit(kind, src)
    local r = PCSX.getRegisters()
    csv:row(
        "%s,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,"
        .. "0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X",
        kind, src, tonumber(r.pc) or 0,
        reg(r, "ra"), reg(r, "a0"), reg(r, "a1"), reg(r, "a2"), reg(r, "a3"),
        reg(r, "v0"), reg(r, "v1"),
        reg(r, "t0"), reg(r, "t1"), reg(r, "t2"), reg(r, "t3"),
        reg(r, "t4"), reg(r, "t5"), reg(r, "t6"), reg(r, "t7"),
        reg(r, "s0"), reg(r, "s1"), reg(r, "s2"), reg(r, "s3"))
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),
    hold_button    = HOLD_BUTTON ~= 0 and HOLD_BUTTON or nil,
    hold_frames    = HOLD_FRAMES,

    on_arm = function()
        local descs = {}

        -- (1) Read bps tiled across the slot-4 window.
        local off = 0
        while off < SLOT4_SIZE do
            local addr = SLOT4_BASE + off
            local d = { addr = addr, off = off, n = 0, capped = false, first = true }
            probe.arm_breakpoint(addr, "Read", 4,
                string.format("S4_+0x%05X", off), function()
                    d.n = d.n + 1
                    if d.n > MAX_HITS then
                        if not d.capped then
                            PCSX.log(string.format(
                                "[s4map] read +0x%05X cap reached", d.off))
                            d.capped = true
                        end
                        return
                    end
                    emit("rd", addr)
                    if d.first then
                        d.first = false
                        local snap = probe.capture_call_context(string.format(
                            "slot4 read +0x%05X (0x%08X) first hit", d.off, addr))
                        probe.append_call_context(DETAIL_PATH, snap)
                    end
                end)
            descs[#descs + 1] = d
            off = off + STRIDE
        end

        -- (2) Exec bp on the streaming-chunk dispatcher entry.
        local disp = { n = 0 }
        probe.arm_breakpoint(FUN_8001E54C, "Exec", 4, "FUN_8001E54C", function()
            disp.n = disp.n + 1
            emit("disp", 0)
            if disp.n <= 8 then
                local r = PCSX.getRegisters()
                local a0 = reg(r, "a0")
                PCSX.log(string.format(
                    "[s4map] dispatch %d: a0=0x%08X w0=0x%08X w1=0x%08X",
                    disp.n, a0, read_u32(a0), read_u32(a0 + 4)))
            end
        end)

        PCSX.log(string.format(
            "[s4map] %d read bps across slot-4 [0x%08X..0x%08X) + dispatcher exec bp",
            #descs, SLOT4_BASE, SLOT4_BASE + SLOT4_SIZE))
        return { descs = descs, disp = disp }
    end,

    on_done = function(_, state)
        csv:close()
        PCSX.log("[s4map] CSV closed: " .. OUT_PATH)
        local hit = 0
        for _, d in ipairs(state.descs) do
            if d.n > 0 then hit = hit + 1 end
        end
        PCSX.log(string.format(
            "[s4map] %d/%d slot-4 read bps fired; %d dispatcher calls",
            hit, #state.descs, state.disp.n))
    end,
})
