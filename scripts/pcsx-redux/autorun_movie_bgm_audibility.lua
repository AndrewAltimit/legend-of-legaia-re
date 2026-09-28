-- autorun_movie_bgm_audibility.lua
--
-- Is the field score audible under a mid-game movie? Retail's movie path
-- (master mode 0x1A: FUN_80025FB4 / FUN_801CEA3C / FUN_801CF098) makes no
-- BGM-slot or SsSeq* call, and the sequencer is clocked from the root-counter
-- callback, so whatever the script left sounding may keep sounding under the
-- movie's XA. This probe measures it: it streams the SPU sub-message (per-voice
-- ADSR envelope level) before and after a movie starts, while logging the
-- master-mode word and the BGM globals every vsync.
--
-- Two ways to start the movie, both at vsync LEGAIA_TRIGGER_V after the load:
--   LEGAIA_FMV=N           make the two stores field-VM op `4C E2` makes
--                          (sh N -> 0x8007BA78, sh 0x1A -> 0x8007B83C;
--                          handler 0x801E30E4..0x801E3104 in PROT 0897) -
--                          the op itself, without the script around it.
--   LEGAIA_POKE_POS=x,z    write the player's +0x14/+0x18 (a walk-on trigger
--                          tile), then hold LEGAIA_WALK (pad button name,
--                          default UP) for LEGAIA_WALK_V vsyncs, so the scene's
--                          own trigger record runs its own op-0x35 words and
--                          its own `4C E2`.
-- Neither set = a baseline run (no movie), or an organic one: with
-- LEGAIA_MASH_EVERY=N the probe taps CROSS every N vsyncs until the first
-- mode-0x1A vsync, so a state parked before a scripted beat (a post-battle
-- transition, a conversation) plays forward into the scene's own trigger
-- record - its own op-0x35 words, then its own `4C E2`.
-- LEGAIA_SHOT_EVERY=N adds a screenshot every N vsyncs.
--
-- An op-0x35 sub-op arm is a flag store plus one SCUS call, so a scene whose
-- record runs one before its trigger can be emulated from any field state:
--   LEGAIA_CALL=addr:a0[:a1]  at the first field tick (FUN_8001698C entry) at
--                             or after LEGAIA_CALL_V, run that function with
--                             those arguments and return into the tick, which
--                             then runs as usual (ra is parked on the tick's
--                             entry and restored on the second hit);
--   LEGAIA_FLAGS_OR=hex       OR this into _DAT_8007B750 at the same moment.
-- e.g. sub-op 2 (arm 0x801E0138): LEGAIA_FLAGS_OR=2
-- LEGAIA_CALL=0x800266E0:0x8007052C. LEGAIA_CALL needs breakpoints - run it
-- without --fast.
--
-- Outputs in LEGAIA_OUT_DIR (default captures/movie_bgm_audibility):
--   modes.csv  v, mode (0x8007B83C), fmv (0x8007BA78), bgm_id (0x8007BAC8),
--              flags (0x8007B750), scene - one row per change
--   shot_NNNN.raw  screenshots at the LEGAIA_SHOTS vsyncs ("200,400"), for
--              raw2png.py - the evidence that a movie frame is on screen.
--   spu.bin    LEGSPU01 stream, one frame every LEGAIA_SPU_EVERY vsyncs plus
--              one on every master-mode change; decode with
--              extract_audio_trace_from_sstates.py.
--
-- Vsync-driven, no breakpoints: run with --fast.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")
local spu = require("probe.spu")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 600)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "captures/movie_bgm_audibility")
local TRIG_V = probe.getenv_num("LEGAIA_TRIGGER_V", 60)
local FMV = probe.getenv_num("LEGAIA_FMV", -1)
local EVERY = math.max(1, probe.getenv_num("LEGAIA_SPU_EVERY", 10))
local WALK = pad.BTN[probe.getenv("LEGAIA_WALK", "UP")] or pad.BTN.UP
local WALK_V = probe.getenv_num("LEGAIA_WALK_V", 30)
local MASH_EVERY = probe.getenv_num("LEGAIA_MASH_EVERY", 0)
local SHOT_EVERY = probe.getenv_num("LEGAIA_SHOT_EVERY", 0)
local movie_seen = false
local CALL_V = probe.getenv_num("LEGAIA_CALL_V", 40)
local FLAGS_OR = tonumber(probe.getenv("LEGAIA_FLAGS_OR", "0")) or 0
local CALL = nil
do
    local f, x, y = string.match(probe.getenv("LEGAIA_CALL", ""), "^(0?x?%x+):(0?x?%x+):?(0?x?%x*)$")
    if f then CALL = { fn = tonumber(f), a0 = tonumber(x), a1 = tonumber(y ~= "" and y or "0") } end
end
local FIELD_TICK = 0x8001698C
local call_stage, call_ra, cur_v = 0, 0, 0
local SHOTS = {}
for t in string.gmatch(probe.getenv("LEGAIA_SHOTS", ""), "%d+") do SHOTS[tonumber(t)] = true end
local POKE = nil
do
    local px, pz = string.match(probe.getenv("LEGAIA_POKE_POS", ""), "(%-?%d+),(%-?%d+)")
    if px then POKE = { tonumber(px), tonumber(pz) } end
end
os.execute(string.format("mkdir -p %q", OUT_DIR))
local CSV = io.open(OUT_DIR .. "/modes.csv", "w")
CSV:write("v,mode,fmv,bgm_id,flags,scene,b880\n")
local W = spu.open(OUT_DIR .. "/spu.bin")

local function scene()
    local t = {}
    for i = 0, 7 do
        local b = probe.read_u8(0x8007050C + i) or 0
        if b < 0x20 or b >= 0x7F then break end
        t[#t + 1] = string.char(b)
    end
    return table.concat(t)
end

-- Screenshot (raw + .raw.meta for raw2png.py): proves a movie frame is up.
local function shot(v)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not (ok and ss) then PCSX.log("[movie_bgm] shot failed: " .. tostring(ss)); return end
    -- ss.bpp reads 16 over a 24-bit MDEC frame; the byte count does not lie.
    local data = tostring(ss.data)
    local bpp = (#data >= tonumber(ss.width) * tonumber(ss.height) * 3) and 24 or 16
    local base = string.format("%s/shot_%04d.raw", OUT_DIR, v)
    local h = io.open(base, "wb"); h:write(data); h:close()
    local m = io.open(base .. ".meta", "w")
    m:write(string.format("width=%d\nheight=%d\nbpp=%d\n", tonumber(ss.width), tonumber(ss.height), bpp))
    m:close()
end

local last, last_mode, walking_until = nil, nil, nil
probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        if not CALL then return {} end
        local d = { addr = FIELD_TICK, name = "op35_call", hits_ref = { n = 0 } }
        probe.arm_breakpoint(FIELD_TICK, "Exec", 4, d.name, function()
            local r = PCSX.getRegisters()
            if call_stage == 0 and cur_v >= CALL_V then
                call_ra = bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF)
                r.GPR.n.ra = FIELD_TICK
                r.GPR.n.a0 = CALL.a0
                r.GPR.n.a1 = CALL.a1
                r.pc = CALL.fn
                if FLAGS_OR ~= 0 then
                    probe.write_u32(0x8007B750, bit.bor(probe.read_u32(0x8007B750) or 0, FLAGS_OR))
                end
                call_stage = 1
                d.hits_ref.n = d.hits_ref.n + 1
                PCSX.log(string.format("[movie_bgm] v%d: call 0x%08X(0x%X, 0x%X), flags |= 0x%X",
                    cur_v, CALL.fn, CALL.a0, CALL.a1, FLAGS_OR))
            elseif call_stage == 1 then
                r.GPR.n.ra = call_ra
                call_stage = 2
                PCSX.log(string.format("[movie_bgm] v%d: call returned", cur_v))
            end
        end)
        return { d }
    end,
    on_capture = function(_ctx, v)
        cur_v = v
        if v == TRIG_V then
            if FMV >= 0 then
                probe.write_u16(0x8007BA78, FMV)
                probe.write_u16(0x8007B83C, 0x1A)
                PCSX.log(string.format("[movie_bgm] v%d: op 4C E2 stores, fmv %d", v, FMV))
            elseif POKE then
                local p = probe.read_u32(0x8007C364) or 0
                if p >= 0x80000000 and p < 0x80200000 then
                    probe.write_u16(p + 0x14, POKE[1] % 0x10000)
                    probe.write_u16(p + 0x18, POKE[2] % 0x10000)
                    pad.force(WALK)
                    walking_until = v + WALK_V
                    PCSX.log(string.format("[movie_bgm] v%d: player -> (%d,%d), walking", v, POKE[1], POKE[2]))
                end
            end
        end
        if walking_until and v >= walking_until then pad.release(WALK); walking_until = nil end
        local mode = probe.read_u16(0x8007B83C) or 0
        if mode == 0x1A then movie_seen = true end
        if MASH_EVERY > 0 and not movie_seen then
            local ph = v % MASH_EVERY
            if ph == 0 then pad.force(pad.BTN.CROSS) elseif ph == 4 then pad.release(pad.BTN.CROSS) end
        elseif MASH_EVERY > 0 and movie_seen then
            pad.release(pad.BTN.CROSS)
        end
        local key = string.format("%X,%d,%d,%X,%s", mode,
            probe.read_u16(0x8007BA78) or 0, probe.read_u32(0x8007BAC8) or 0,
            probe.read_u32(0x8007B750) or 0, scene()) .. string.format(",%X", probe.read_u32(0x8007B880) or 0)
        if key ~= last then
            CSV:write(string.format("%d,%s\n", v, key)); CSV:flush(); last = key
        end
        if v % EVERY == 0 or mode ~= last_mode then
            local ok, err = spu.capture(W, v)
            if not ok then PCSX.log("[movie_bgm] spu capture threw: " .. tostring(err)) end
        end
        if SHOTS[v] or (SHOT_EVERY > 0 and v % SHOT_EVERY == 0) then shot(v) end
        last_mode = mode
    end,
    on_done = function()
        if walking_until then pad.release(WALK) end
        spu.close(W)
        CSV:close()
        PCSX.log(string.format("=== movie_bgm_audibility done (%d spu frames) ===", W.n))
    end,
})
