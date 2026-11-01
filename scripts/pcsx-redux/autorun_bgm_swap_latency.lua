-- autorun_bgm_swap_latency.lua
--
-- How long a mid-scene BGM start takes to sound: the swap poller
-- FUN_800243F0's stage walk from the request to the slot re-attach.
--
-- At vsync LEGAIA_TRIGGER_V after the load the probe makes the one store a
-- field-VM op-0x35 sub-op 1 start makes - the track word 0x8007BAC8 = id
-- (arm 0x801E0254 in PROT 0897) - so the requested bank index the poller
-- derives from it (0x80024424..0x80024460) parts from the loaded one and the
-- real poller runs on retail's own CD timing. Then it logs one row per vsync:
--
--   swap.csv  v, frame (gp+0x480 = 0x8007B798), step (0x1F800393),
--             stage (gp+0x744 = 0x8007BA5C), settle (gp+0x768 = 0x8007BA80),
--             word (0x8007BAC8), req (0x8007BAB8), loaded (0x8007BA9C),
--             flags (0x8007B750), cdmode (0x8007B83C), cdread (0x8007B8C2),
--             src_open / src_seq (field-BGM source 0x8007052C +0x8 / +0xA),
--             seq_flags (that libsnd sequence's +0x98, bit 0 = playing)
--
-- The stage table (JT 0x800108C8): 0 wait CD idle and arm the settle,
-- 1 wait CD idle, 2 tear down + kick the payload read FUN_8001FC00, 3 wait
-- the read, 4 wait the settle (then set bit 3 and run the handshake), 5
-- install the SEQ FUN_8001E54C, 6 re-attach the slot FUN_80026478 and latch
-- loaded = requested. The settle counts down by the frame step on every
-- call, from the stage-0 pass on (0x80024880..0x80024894).
--
-- Env:
--   LEGAIA_SSTATE      state (run_probe.sh --scenario sets it)
--   LEGAIA_TRIGGER_V   vsync of the store (default 120)
--   LEGAIA_BGM_ID      track word to store (default 2001)
--   LEGAIA_FRAMES      vsyncs to run (default 400)
--
-- Vsync-driven, no breakpoints: run with --fast. The store is a probe poke;
-- everything after it is retail's.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 400)
local TRIG_V = probe.getenv_num("LEGAIA_TRIGGER_V", 120)
local BGM_ID = probe.getenv_num("LEGAIA_BGM_ID", 2001)

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end

local out
local fired = false

probe.run({
    sstate = SSTATE, capture_frames = FRAMES,
    on_arm = function()
        out = probe.csv_open(probe.out_path("swap.csv"),
            "v,frame,step,stage,settle,word,req,loaded,flags,cdmode,cdread,src_open,src_seq,seq_flags")
        PCSX.log(string.format("[bgm-swap] trigger v%d id %d", TRIG_V, BGM_ID))
        return {}
    end,
    on_capture = function(c, elapsed)
        if not fired and elapsed >= TRIG_V then
            fired = true
            PCSX.log(string.format("[bgm-swap v%d] word %d -> %d (req %d loaded %d)",
                elapsed, u32(0x8007BAC8), BGM_ID, u32(0x8007BAB8), u32(0x8007BA9C)))
            probe.write_u32(0x8007BAC8, BGM_ID)
        end
        local seq = u16(0x8007052C + 0xA)
        local rec = u32(0x801CD2C0 + (seq % 32) * 4)
        local sflags = -1
        if rec >= 0x80000000 and rec < 0x80200000 then sflags = u32(rec + 0x98) end
        local settle = u32(0x8007BA80)
        if settle >= 0x80000000 then settle = settle - 0x100000000 end
        out:row("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
            elapsed, u32(0x8007B798), probe.read_scratch_u8(0x1F800393), u32(0x8007BA5C), settle,
            u32(0x8007BAC8), u32(0x8007BAB8), u32(0x8007BA9C), u32(0x8007B750),
            u16(0x8007B83C), u16(0x8007B8C2), u16(0x8007052C + 8), seq, sflags)
    end,
    on_done = function()
        if out then out:close() end
        PCSX.quit(0)
    end,
})
