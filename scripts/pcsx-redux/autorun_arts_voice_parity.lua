-- autorun_arts_voice_parity.lua
--
-- The retail side of the arts-voice parity check: type a Tactical Arts string
-- through the pad on an arts-command-input state, run the turn, and log every
-- voice / sound cue the turn fires with the battle clock it fired on.
--
-- Breakpoints (each row carries the vsync since arm, `ra`, and the battle
-- context's action-SM state `ctx[+0x07]`, acting seat `ctx[+0x13]`, strike
-- cursor `ctx[+0x15]` and solo latch `ctx[+0x243]` at the hit):
--   * FUN_8004C140  arts-voice selector (char_id a0, action a1, flag a2)
--   * FUN_8003D53C  CD-XA clip start (clip slot a0, channel a1, dur a2)
--   * FUN_8004FCC8  cue dispatcher (id a0)
-- plus one `seat` row per change of a party actor's committed clip `+0x1D9`,
-- so a cue can be placed on the clip timeline the engine commits on.
--
-- Input letters (LEGAIA_INPUT_SEQ), one per PRESS+GAP step:
--   U D L R  d-pad;  S START;  X T O Q  Cross / Triangle / Circle / Square;
--   .        an idle step (no button).
-- Edge-only pad override (the autorun_super_art_input_replay.lua pattern).
--
-- Run:
--   LEGAIA_INPUT_SEQ="UDUUDDS" LEGAIA_FRAMES=1500 \
--   xvfb-run -a timeout --kill-after=20s 1200s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --scenario arts_bar_ideal_noa_fangs \
--       --lua scripts/pcsx-redux/autorun_arts_voice_parity.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 1500)
local SEQ         = probe.getenv("LEGAIA_INPUT_SEQ", "")
local START_DELAY = probe.getenv_num("LEGAIA_START_DELAY", 60)
local PRESS       = probe.getenv_num("LEGAIA_PRESS", 4)
local GAP         = probe.getenv_num("LEGAIA_GAP", 8)
local OUT_PATH    = probe.out_path("arts_voice_parity.csv")
-- Optional screenshot every N vsyncs after arm (0 = none), for steering a
-- new input sequence.
local SHOT_EVERY  = probe.getenv_num("LEGAIA_SHOT_EVERY", 0)
-- Optional per-vsync `tick` rows for one party seat (-1 = none): the seat's
-- rate byte `+0x21D`, committed / staged clip `+0x1D9` / `+0x1DA`, the
-- frame step `*(0x1F800393)`, the action-SM state, and in the last column the
-- main loop's frame counter `0x8007B798`, the seat's anim cursor `+0x17A`, and
-- the CPU's `pc` / `ra` at the vsync (a sampling profile of where frames go).
local TRACE_SEAT  = probe.getenv_num("LEGAIA_TRACE_SEAT", -1)

local ACTOR_TABLE = 0x801C9370 -- 8 x u32 battle-actor pointers; 0..2 = party
local CTX_PTR     = 0x8007BD24 -- battle-action context
local GMODE       = 0x8007B83C

local BTN_FOR = {
    U = probe.BTN.UP,   D = probe.BTN.DOWN,
    L = probe.BTN.LEFT, R = probe.BTN.RIGHT,
    S = probe.BTN.START,
    X = probe.BTN.CROSS, T = probe.BTN.TRIANGLE,
    O = probe.BTN.CIRCLE, Q = probe.BTN.SQUARE,
}

local inputs = {}
for i = 1, #SEQ do
    local ch = SEQ:sub(i, i):upper()
    if ch ~= "." and BTN_FOR[ch] == nil then
        error(string.format("LEGAIA_INPUT_SEQ char %d %q unknown (UDLRSXTOQ.)", i, ch))
    end
    inputs[#inputs + 1] = { ch = ch, btn = BTN_FOR[ch] }
end

local function n32(v) return (tonumber(v) or 0) % 0x100000000 end
local function u8(a) return probe.read_u8(a) or 0xFF end

local csv = probe.csv_open(OUT_PATH, "frame,kind,a0,a1,a2,ra,sm,seat,cursor,latch,queue")
local armed, arm_frame, now = false, 0, 0
local last_clip = { -1, -1, -1 }

local function ctx_bytes()
    local c = n32(probe.read_u32(CTX_PTR))
    if not probe.in_ram(c) then return 0xFF, 0xFF, 0xFF, 0xFF end
    return u8(c + 0x07), u8(c + 0x13), u8(c + 0x15), u8(c + 0x243)
end

local function queue_of(seat)
    local p = n32(probe.read_u32(ACTOR_TABLE + seat * 4))
    if not probe.in_ram(p) then return "" end
    local b = probe.read_bytes(p + 0x1DF, 0x14)
    return b and probe.bytes_to_hex(b):gsub("%s+", "") or ""
end

local function row(kind, a0, a1, a2)
    local r = PCSX.getRegisters()
    local ra = n32(r.GPR.n.ra)
    local sm, seat, cur, latch = ctx_bytes()
    local q = (seat ~= 0xFF and seat < 3) and queue_of(seat) or ""
    PCSX.log(string.format("[avp] f%d %s a0=0x%X a1=0x%X a2=0x%X ra=0x%08X sm=0x%02X seat=%d cur=%d latch=%d q=%s",
        now, kind, a0, a1, a2, ra, sm, seat, cur, latch, q))
    csv:row("%d,%s,0x%X,0x%X,0x%X,0x%08X,0x%02X,%d,%d,%d,%s",
        now, kind, a0, a1, a2, ra, sm, seat, cur, latch, q)
end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data))
    fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 228, (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
end

local forced_btn = nil
local function pad_set(btn)
    if forced_btn == btn then return end
    if forced_btn ~= nil then probe.pad_release(forced_btn) end
    if btn ~= nil then probe.pad_force(btn) end
    forced_btn = btn
end

local STEP = PRESS + GAP
local function drive_input(rel)
    if rel < 0 then return end
    if rel >= #inputs * STEP then pad_set(nil) return end
    local i = math.floor(rel / STEP) + 1
    local sub = rel % STEP
    if sub < PRESS and inputs[i].btn ~= nil then
        if sub == 0 then PCSX.log(string.format("[avp] f%d input %d %q", now, i, inputs[i].ch)) end
        pad_set(inputs[i].btn)
    else
        pad_set(nil)
    end
end

local function arm_bps()
    probe.arm_breakpoint(0x8004C140, "Exec", 4, "arts_voice_sel", function()
        local r = PCSX.getRegisters()
        row("select", n32(r.GPR.n.a0), n32(r.GPR.n.a1), n32(r.GPR.n.a2))
    end)
    probe.arm_breakpoint(0x8003D53C, "Exec", 4, "xa_clip", function()
        local r = PCSX.getRegisters()
        row("clip", n32(r.GPR.n.a0), n32(r.GPR.n.a1), n32(r.GPR.n.a2))
    end)
    probe.arm_breakpoint(0x8004FCC8, "Exec", 4, "cue", function()
        local r = PCSX.getRegisters()
        row("cue", n32(r.GPR.n.a0), n32(r.GPR.n.a1), 0)
    end)
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function() return {} end,

    on_capture = function(hctx, elapsed)
        if not armed then
            if not probe.in_ram(n32(probe.read_u32(ACTOR_TABLE))) then return end
            armed, arm_frame = true, elapsed
            PCSX.log(string.format("[avp] armed f%d gmode=0x%02X seq=%q", elapsed, u8(GMODE), SEQ))
            arm_bps()
        end
        now = elapsed - arm_frame
        drive_input(now - START_DELAY)
        if SHOT_EVERY > 0 and now % SHOT_EVERY == 0 then
            screenshot(string.format("shot_%05d", now))
        end
        if TRACE_SEAT >= 0 then
            local p = n32(probe.read_u32(ACTOR_TABLE + TRACE_SEAT * 4))
            if probe.in_ram(p) then
                local sm, s, cur, latch = ctx_bytes()
                local step = probe.read_scratch_u8(0x1F800393) or 0xFF
                local fc = n32(probe.read_u32(0x8007B798))
                local ac = probe.read_u16(p + 0x17A) or 0
                local regs = PCSX.getRegisters()
                csv:row("%d,tick,0x%02X,0x%02X,0x%02X,%d,0x%02X,%d,%d,%d,f%d/c%d/pc%08X/ra%08X",
                    now, u8(p + 0x21D), u8(p + 0x1D9), u8(p + 0x1DA), step, sm, s, cur, latch, fc, ac,
                    n32(regs.pc), n32(regs.GPR.n.ra))
            end
        end
        for seat = 0, 2 do
            local p = n32(probe.read_u32(ACTOR_TABLE + seat * 4))
            if probe.in_ram(p) then
                local clip = u8(p + 0x1D9)
                if clip ~= last_clip[seat + 1] then
                    last_clip[seat + 1] = clip
                    local sm, s, cur, latch = ctx_bytes()
                    csv:row("%d,seat,%d,0x%02X,0,0,0x%02X,%d,%d,%d,%s",
                        now, seat, clip, sm, s, cur, latch, queue_of(seat))
                end
            end
        end
    end,

    on_done = function()
        pad_set(nil)
        csv:close()
        PCSX.log("=== arts-voice parity probe done ===")
    end,
})
