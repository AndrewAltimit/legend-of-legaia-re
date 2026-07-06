-- autorun_super_art_input_replay.lua
--
-- Batch-capture rig for the remaining Super Arts' action-queue `replace`
-- strings (A3). Companion to autorun_super_art_action_queue.lua: that probe
-- reads a queue already resident in the save state; THIS one loads an
-- arts-command-input battle state (game_mode 0x15, gauge open), INJECTS the
-- Super's directional command string through the pad override, commits it,
-- and polls the party actors' action-parameter streams at
-- actor[+0x1DF..+0x1F2] every frame, logging every change. The find/replace
-- expansion (super_art.rs `replace`) appears as the tail of the inputting
-- actor's stream once the input is committed.
--
-- Input sequence letters (LEGAIA_INPUT_SEQ):
--   U D L R  = d-pad directions (the arts command inputs)
--   S        = START     (commit a partially-filled gauge)
--   X T O Q  = Cross / Triangle / Circle / Square (arm-limb inputs; avoid --
--              an arm block after the last art breaks the tail match)
--
-- Timing: each letter is held LEGAIA_PRESS frames then released for
-- LEGAIA_GAP frames (defaults 4/8, the cadence autorun_arts_input_press.lua
-- validated on the same states). Injection starts LEGAIA_START_DELAY frames
-- after the party actor table resolves.
--
-- Run (one Super per run; see docs/tooling/super-art-queue-capture.md):
--   LEGAIA_INPUT_SEQ="DULULUUDUDDS" LEGAIA_FRAMES=1400 \
--   xvfb-run -a timeout --kill-after=15s 900s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --scenario arts_input_start_gala_nail \
--       --lua scripts/pcsx-redux/autorun_super_art_input_replay.lua
--
-- Output CSV rows: frame,slot,actor,hex(+0x1DF..+0x1F2). Row frame=-1 is the
-- arm-time snapshot of +0x1D8..+0x200 (wider window, incl. +0x1DE category).
-- Offline check: the last rows' tail vs the Super's `replace` bytes.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 1400)
local SEQ         = probe.getenv("LEGAIA_INPUT_SEQ", "")
local START_DELAY = probe.getenv_num("LEGAIA_START_DELAY", 90)
local PRESS       = probe.getenv_num("LEGAIA_PRESS", 4)
local GAP         = probe.getenv_num("LEGAIA_GAP", 8)
-- After the sequence finishes: quit once a queue containing 0x1A has been
-- stable this many frames, or unconditionally POST frames later.
local QUIET       = probe.getenv_num("LEGAIA_QUIET", 180)
local POST        = probe.getenv_num("LEGAIA_POST", 900)
local OUT_PATH    = probe.out_path("super_art_input_replay.csv")

local ACTOR_TABLE = 0x801C9370 -- 8 x u32 battle-actor pointers; 0..2 = party
local GMODE       = 0x8007B83C
local Q_OFF       = 0x1DF
local Q_LEN       = 0x14
local SNAP_LO     = 0x1D8
local SNAP_HI     = 0x200

local BTN_FOR = {
    U = probe.BTN.UP,   D = probe.BTN.DOWN,
    L = probe.BTN.LEFT, R = probe.BTN.RIGHT,
    S = probe.BTN.START,
    X = probe.BTN.CROSS, T = probe.BTN.TRIANGLE,
    O = probe.BTN.CIRCLE, Q = probe.BTN.SQUARE,
}

-- Parse the sequence into a button list up front so a typo dies loudly.
local inputs = {}
for i = 1, #SEQ do
    local ch = SEQ:sub(i, i):upper()
    if BTN_FOR[ch] == nil then
        error(string.format("LEGAIA_INPUT_SEQ char %d %q unknown (UDLRSXTOQ)", i, ch))
    end
    inputs[#inputs + 1] = { ch = ch, btn = BTN_FOR[ch] }
end

local function party_actor(slot)
    local p = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
    return p % 0x100000000
end

local function qhex(p, lo, hi)
    local b = probe.read_bytes(p + lo, hi - lo)
    if b == nil then return "" end
    return probe.bytes_to_hex(b):gsub("%s+", "")
end

local csv = probe.csv_open(OUT_PATH, "frame,slot,actor,queue_1df_hex")
local armed = false
local arm_frame = 0
local last_hex = { "", "", "" }
local seq_done_frame = nil
local stable_since = nil
local stable_hex = nil

local STEP = PRESS + GAP
local SEQ_FRAMES = #inputs * STEP

-- Edge-only pad driving (the autorun_replay_inputs.lua pattern): issue
-- setOverride / clearOverride ONLY on transitions. Blanket release-all every
-- frame segfaults this PCSX-Redux build ~150 frames in on the arts-input
-- states; edge-only is the call pattern the long S5 input replay validated.
local forced_btn = nil

local function pad_set(btn)
    if forced_btn == btn then return end
    if forced_btn ~= nil then probe.pad_release(forced_btn) end
    if btn ~= nil then probe.pad_force(btn) end
    forced_btn = btn
end

local function drive_input(rel)
    -- rel = frames since injection start; press inputs[i] for PRESS frames
    -- at rel in [ (i-1)*STEP, (i-1)*STEP+PRESS ).
    if rel < 0 then return false end
    if rel >= SEQ_FRAMES then
        pad_set(nil)
        return true
    end
    local i = math.floor(rel / STEP) + 1
    local sub = rel % STEP
    if sub < PRESS then
        if sub == 0 then
            PCSX.log(string.format("[sai] input %d/%d %q", i, #inputs, inputs[i].ch))
        end
        pad_set(inputs[i].btn)
    else
        pad_set(nil)
    end
    return false
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        PCSX.log(string.format("[sai] seq=%q (%d inputs, %d frames); deferring to post-load",
            SEQ, #inputs, SEQ_FRAMES))
        return {}
    end,

    on_capture = function(hctx, elapsed)
        if not armed then
            local p0 = party_actor(0)
            if not probe.in_ram(p0) then return end
            armed = true
            arm_frame = elapsed
            PCSX.log(string.format("[sai] armed at frame %d gmode=0x%02X", elapsed,
                probe.read_u8(GMODE) or 0))
            for slot = 0, 2 do
                local p = party_actor(slot)
                if probe.in_ram(p) then
                    local snap = qhex(p, SNAP_LO, SNAP_HI)
                    PCSX.log(string.format("[sai] slot %d actor=0x%08X +0x1D8..0x200=%s",
                        slot, p, snap))
                    csv:row("-1,%d,0x%08X,%s", slot, p, snap)
                    last_hex[slot + 1] = qhex(p, Q_OFF, Q_OFF + Q_LEN)
                end
            end
            return
        end

        -- 1) drive the pad
        local rel = elapsed - arm_frame - START_DELAY
        local done = drive_input(rel)
        if done and seq_done_frame == nil then
            seq_done_frame = elapsed
            PCSX.log(string.format("[sai] input sequence done at frame %d", elapsed))
        end

        -- 2) poll the three party queues; log every change
        local any_special = false
        for slot = 0, 2 do
            local p = party_actor(slot)
            if probe.in_ram(p) then
                local hex = qhex(p, Q_OFF, Q_OFF + Q_LEN)
                if hex ~= last_hex[slot + 1] then
                    last_hex[slot + 1] = hex
                    csv:row("%d,%d,0x%08X,%s", elapsed, slot, p, hex)
                    PCSX.log(string.format("[sai] f%d slot%d +0x1DF=[%s]", elapsed, slot, hex))
                end
                if hex:find("1a", 1, true) or hex:find("1A", 1, true) then
                    any_special = true
                end
            end
        end

        -- 3) quit logic (only after the sequence has been fully injected)
        if seq_done_frame == nil then return end
        local cur = table.concat(last_hex, "|")
        if cur ~= stable_hex then
            stable_hex = cur
            stable_since = elapsed
        elseif any_special and elapsed - stable_since >= QUIET then
            PCSX.log("[sai] queue stable with SpecialStarter present; quitting")
            hctx.request_quit = true
        end
        if elapsed - seq_done_frame >= POST then
            PCSX.log("[sai] POST budget exhausted; quitting")
            hctx.request_quit = true
        end
    end,

    on_done = function()
        csv:close()
        for slot = 0, 2 do
            PCSX.log(string.format("[sai] final slot %d +0x1DF=[%s]", slot, last_hex[slot + 1]))
        end
        PCSX.log(string.format("=== super-art input-replay probe done (armed=%s seq=%q) ===",
            tostring(armed), SEQ))
    end,
})
