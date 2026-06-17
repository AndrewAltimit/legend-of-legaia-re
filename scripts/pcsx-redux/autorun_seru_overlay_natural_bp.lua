-- autorun_seru_overlay_natural_bp.lua
--
-- NATURAL-breakpoint diagnostic for the seru-trade overlay slice. Unlike the
-- earlier injected-code probes (autorun_seru_overlay_*_slice.lua), this arms a
-- *natural* execution breakpoint at the op-0x49 arm-edge detour site and lets
-- the game reach it on its own -- so the PCSX-Redux stale-decode limitation
-- (debugger-written code runs from a cold decode cache) does NOT apply: the
-- code under test came off the patched disc via CD-DMA, which is coherent.
--
-- It answers the one question static RE can't: when op-0x49 arms, does our
-- detour actually execute, and what is the sub-op the stub gates on?
--
-- Use the UNGATED diagnostic disc so the detour fires on the FIRST op-0x49 arm
-- of any kind (New Game name-entry is the earliest), not only shop sub-op 0:
--   cargo run -p legaia-rando --example overlay_slice_bin -- \
--       <input.bin> /tmp/legaia_slice_ungated.bin   # with LEGAIA_SLICE_UNGATED=1
--
-- Run (cold boot; play to ANY menu -- e.g. start New Game, reach name entry):
--   LEGAIA_SLICE_UNGATED=1 ... build the disc ...
--   timeout --kill-after=30s 1800s bash scripts/pcsx-redux/run_probe.sh \
--     --iso /tmp/legaia_slice_ungated.bin \
--     --lua scripts/pcsx-redux/autorun_seru_overlay_natural_bp.lua
--
-- Optional: set LEGAIA_SSTATE to a PCSX-Redux field save state to skip the
-- cold-boot navigation (the probe loads it once at boot if the file exists).
--
-- What it reports when the hook fires:
--   instr@0x801E09A8 == 0x0801EB80 (j 0x8007AE00)?  -> detour reached RAM
--   sub-op (*s6)                                     -> what the gate sees
--   then whether the stub / overlay-dest / return run, and the sentinel value.
--
-- Reading those four together splits the two surviving hypotheses:
--   * instr != detour word  -> the patched field overlay never reached RAM
--     (loaded copy != the PROT entry we patch) -- a load/identity problem.
--   * instr == detour, stub/dest run, sentinel set -> the mechanism works;
--     the earlier gated failure was the sub-op (Variety Store sub-op != 0).
--   * instr == detour but stub/dest never run -> the detour jump itself fails
--     at runtime (alignment / cache) -- a stub problem.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local HOOK       = 0x801E09A8 -- op-0x49 arm-edge detour site
local STUB       = 0x8007AE00 -- loader stub entry
local DEST       = 0x801F69D8 -- overlay load + exec address
local RETURN     = 0x801E09B0 -- stub return into the dispatcher
local SENT_ADDR  = 0x8007AF20
local SENTINEL   = 0x5E2D7ADE
local DETOUR_W   = 0x0801EB80 -- j 0x8007AE00 (the word the detour writes at HOOK)

local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 180) -- vsyncs before arming
local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 72000)   -- ~20 min play window
local QUIT_AFTER = probe.getenv_num("LEGAIA_QUIT_AFTER", 120) -- vsyncs after first hit
local SSTATE     = os.getenv("LEGAIA_SSTATE")

local st = {
    hook = 0, stub = 0, dest = 0, ret = 0,
    instr = nil, subop = nil, s6 = nil,
    sent_pre = nil, sent_post = nil,
    captured = false, hit_vsync = nil,
}

local function read_subop()
    local r = PCSX.getRegisters()
    local s6 = r.GPR.n.s6
    return s6, probe.read_u8(s6)
end

local function arm_all()
    -- The primary natural breakpoint: the detour site itself.
    probe.arm_breakpoint(HOOK, "Exec", 4, "hook", function()
        st.hook = st.hook + 1
        if not st.captured then
            st.captured = true
            st.instr = probe.read_u32(HOOK)
            st.s6, st.subop = read_subop()
            st.sent_pre = probe.read_u32(SENT_ADDR)
            PCSX.log(string.format(
                "[nat] HOOK fired: instr@0x%08X=0x%08X (detour=%s)  s6=0x%08X *s6(sub-op)=0x%02X  sentinel-pre=0x%08X",
                HOOK, st.instr, tostring(st.instr == DETOUR_W),
                st.s6 or 0, st.subop or 0, st.sent_pre or 0))
        end
    end)
    -- Did execution actually reach the stub / overlay / return?
    probe.arm_breakpoint(STUB, "Exec", 4, "stub", function()
        st.stub = st.stub + 1
        if st.stub == 1 then PCSX.log("[nat] STUB reached (detour jump worked)") end
    end)
    probe.arm_breakpoint(DEST, "Exec", 4, "dest", function()
        st.dest = st.dest + 1
        if st.dest == 1 then PCSX.log("[nat] DEST reached (overlay executing)") end
    end)
    probe.arm_breakpoint(RETURN, "Exec", 4, "ret", function()
        st.ret = st.ret + 1
        if st.ret == 1 then PCSX.log("[nat] RETURN reached (dispatcher resumed)") end
    end)
end

local function report()
    st.sent_post = probe.read_u32(SENT_ADDR)
    PCSX.log("=== seru overlay NATURAL-BP diagnostic ===")
    PCSX.log(string.format("  HOOK hits:   %d", st.hook))
    PCSX.log(string.format("  STUB hits:   %d", st.stub))
    PCSX.log(string.format("  DEST hits:   %d", st.dest))
    PCSX.log(string.format("  RETURN hits: %d", st.ret))
    if st.instr then
        PCSX.log(string.format("  instr@HOOK:  0x%08X  (detour word 0x%08X -> %s)",
            st.instr, DETOUR_W,
            st.instr == DETOUR_W and "DETOUR PRESENT" or "DETOUR ABSENT"))
        PCSX.log(string.format("  sub-op:      0x%02X  (gate fires the load only when 0 in gated builds)",
            st.subop or 0))
    else
        PCSX.log("  HOOK never fired -- op-0x49 was not reached during the window")
    end
    PCSX.log(string.format("  sentinel:    pre=0x%08X  post=0x%08X  -> %s",
        st.sent_pre or 0, st.sent_post or 0,
        st.sent_post == SENTINEL and "WRITTEN (overlay ran)" or "NOT written"))
    -- Verdict.
    local verdict
    if st.hook == 0 then
        verdict = "INCONCLUSIVE: never reached op-0x49 (play to a menu within the window)"
    elseif st.instr ~= DETOUR_W then
        verdict = "DETOUR ABSENT in RAM: the patched field overlay copy did not load (load/identity problem)"
    elseif st.sent_post == SENTINEL then
        verdict = "MECHANISM WORKS: detour+stub+overlay ran (gated failure was the sub-op gate)"
    elseif st.stub == 0 then
        verdict = "DETOUR PRESENT but jump failed: never reached the stub (alignment/cache)"
    elseif st.dest == 0 then
        verdict = "STUB ran but overlay never executed: CD read / FlushCache / DEST problem"
    else
        verdict = "overlay ran but sentinel unwritten: overlay-store problem"
    end
    PCSX.log("  VERDICT: " .. verdict)
    PCSX.log("=== end ===")
end

-- Cold-boot event loop (we bypass probe.run's mandatory save-state load so the
-- disc boots to the title and the user navigates to a menu themselves).
local vsync = 0
local armed = false
local done_at = nil
local finished = false
PCSX.Events.createEventListener("GPU::Vsync", function()
    vsync = vsync + 1
    if finished then return end
    if not armed then
        if vsync >= BOOT_DELAY then
            if SSTATE and probe.sstate.load and SSTATE ~= "" then
                local ok = pcall(function() return probe.sstate.load(SSTATE) end)
                PCSX.log("[nat] sstate load (" .. SSTATE .. "): " .. tostring(ok))
            end
            arm_all()
            armed = true
            PCSX.log(string.format(
                "[nat] armed natural BP at 0x%08X; play to ANY menu (ungated disc). window=%d vsyncs",
                HOOK, FRAMES))
        end
        return
    end
    if st.captured and not done_at then
        done_at = vsync + QUIT_AFTER -- let the stub/overlay/return BPs settle
    end
    if (done_at and vsync >= done_at) or vsync >= (BOOT_DELAY + FRAMES) then
        finished = true
        probe.bp.disarm()
        report()
        PCSX.quit(0)
    end
end)
PCSX.log("[nat] vsync listener installed; cold boot in progress")
