-- autorun_cast_voice_xa.lua
--
-- The **cast-voice XA channel map**, measured instead of derived: which
-- `XA*.XA` clip slot, which sector-filter channel and which read span retail
-- hands the CD-XA clip starter when a Seru cast starts.
--
-- Three routines, one chain:
--
--   FUN_801F3990  cast audio-cue dispatcher - resolves the cue id from the
--                 acting slot, its char-kind byte `DAT_8007BD10[slot]`, the
--                 cast class `actor[+0x1E8]` and the queue head.
--   FUN_8004FCC8  cue dispatcher - ids `>= 0x100` become
--                 `(slot, channel, duration)` off the table at `DAT_800788B8`,
--                 ids below take the SFX-queue path. TWO decline gates sit
--                 ahead of the XA arm (`[gp+0xA0C]->+0x276 != 0`, and
--                 `FUN_8003DE7C(1) != 0`), so a cue can resolve and still play
--                 nothing.
--   FUN_8003D53C  clip starter - `a0` clip slot, `a1` filter channel,
--                 `a2` duration. This is the triple the port's
--                 `play_xa_clip(clip_slot, channel, duration_sectors)` needs.
--
-- The cast itself is injected the same way `autorun_w3a_cast_oracle.lua`
-- injects one: tap CROSS until a seat's queued category byte lands, then
-- rewrite `actor[+0x1DE] = 2` (Magic) / `+0x1DF = LEGAIA_SPELL` /
-- `+0x1DD = LEGAIA_TARGET_SEAT`. Retail pages the module and runs its own
-- audio dispatch, so everything logged afterwards is retail behaviour.
--
-- Outputs (probe.out_path, i.e. --out-dir):
--   cues.csv    one row per FUN_8004FCC8 entry: cue id, both decline-gate
--               values, the acting slot and its char-kind byte.
--   clips.csv   one row per FUN_8003D53C entry: clip slot, channel, duration,
--               and the `ra` it was reached through.
--   resolve.csv one row per FUN_801F3990 call and return: the five retail
--               inputs and the returned cue.
--
-- Env:
--   LEGAIA_SPELL         action id to force (default 0x83 Vera)
--   LEGAIA_CASTER_SEAT   party seat whose queued action is rewritten (0)
--   LEGAIA_TARGET_SEAT   value written to the caster's +0x1DD (default 3)
--   LEGAIA_PRESS_UNTIL   keep tapping CROSS until this vsync (default 400)
--   LEGAIA_INJECT_AT     vsync to inject on (default 20)
--   LEGAIA_TAIL          vsyncs to keep logging after the last clip (240)
--   LEGAIA_LABEL         free-text label written into manifest.txt
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 1800)
local SPELL       = probe.getenv_num("LEGAIA_SPELL", 0x83)
local CASTER_SEAT = probe.getenv_num("LEGAIA_CASTER_SEAT", 0)
local TARGET_SEAT = probe.getenv_num("LEGAIA_TARGET_SEAT", 3)
local PRESS_UNTIL = probe.getenv_num("LEGAIA_PRESS_UNTIL", 400)
local INJECT_AT   = probe.getenv_num("LEGAIA_INJECT_AT", 20)
local TAIL        = probe.getenv_num("LEGAIA_TAIL", 240)
local LABEL       = probe.getenv("LEGAIA_LABEL", "cast-voice")

local ACTOR_TABLE = 0x801C9370
-- `lw v0, 0xa0c(gp)` at 0x8004FCE0 with the live gp 0x8007B318 IS this
-- pointer, so the dispatcher's first decline gate is `ctx[+0x276] != 0`.
local CTX_PTR     = 0x8007BD24
local CHAR_KIND   = 0x8007BD10   -- DAT_8007BD10[slot]
local CUE_DISPATCH = 0x8004FCC8
local CLIP_START   = 0x8003D53C
local CAST_CUE     = 0x801F3990

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u32(a) return probe.read_u32(a) or 0 end

local function ctxp()
    local c = u32(CTX_PTR)
    if c < 0x80000000 or c >= 0x80200000 then return nil end
    return c
end
local function actor(slot)
    local p = u32(ACTOR_TABLE + slot * 4)
    if p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end
local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function regs()
    local r = PCSX.getRegisters()
    return (r.GPR and r.GPR.n) or {}
end

local cues_csv, clips_csv, resolve_csv
local injected = false
local elapsed_now = 0
local last_clip_vsync = -1
local quit_at = -1
local n_cues, n_clips, n_resolve = 0, 0, 0

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format(
            "== cast voice xa == label=%s spell=0x%02X caster=%d target=%d",
            LABEL, SPELL, CASTER_SEAT, TARGET_SEAT))
        probe.env.write_manifest("autorun_cast_voice_xa.lua", {
            label = LABEL, sstate = SSTATE_PATH, spell = string.format("0x%02X", SPELL),
            caster_seat = CASTER_SEAT, target_seat = TARGET_SEAT, frames = FRAMES,
        })
        cues_csv = probe.csv_open(probe.out_path("cues.csv"),
            "vsync,cue,gate_276,slot,char_kind,ra")
        clips_csv = probe.csv_open(probe.out_path("clips.csv"),
            "vsync,clip_slot,channel,duration,ra")
        resolve_csv = probe.csv_open(probe.out_path("resolve.csv"),
            "vsync,kind,slot,char_kind,cast_class,queue_head,sub_class,v0,ra")

        probe.arm_breakpoint(CUE_DISPATCH, "Exec", 4, "cue_dispatch", function()
            local n = regs()
            local cx = ctxp()
            local slot = cx and u8(cx + 0x13) or -1
            local gate = cx and u8(cx + 0x276) or -1
            n_cues = n_cues + 1
            cues_csv:row("%d,0x%04X,%d,%d,%d,0x%08X",
                elapsed_now, tou32(n.a0), gate, slot,
                slot >= 0 and u8(CHAR_KIND + slot) or -1, tou32(n.ra))
        end)

        probe.arm_breakpoint(CLIP_START, "Exec", 4, "clip_start", function()
            local n = regs()
            n_clips = n_clips + 1
            last_clip_vsync = elapsed_now
            clips_csv:row("%d,%d,%d,%d,0x%08X",
                elapsed_now, tou32(n.a0), tou32(n.a1), tou32(n.a2), tou32(n.ra))
        end)

        probe.arm_breakpoint(CAST_CUE, "Exec", 4, "cast_cue", function()
            local n = regs()
            local cx = ctxp()
            if cx == nil then return end
            local slot = u8(cx + 0x13)
            local a = actor(slot)
            n_resolve = n_resolve + 1
            resolve_csv:row("%d,call,%d,%d,%d,%d,%d,,0x%08X",
                elapsed_now, slot, u8(CHAR_KIND + slot),
                a and u8(a + 0x1E8) or -1, a and u8(a + 0x1DF) or -1,
                a and u8(a + 0x1E9) or -1, tou32(n.ra))
        end)
        return {}
    end,

    on_capture = function(c, elapsed)
        elapsed_now = elapsed
        local cx = ctxp()
        local me = actor(CASTER_SEAT)
        if cx == nil or me == nil then return end

        probe.pad_release(probe.BTN.CROSS)
        if elapsed < PRESS_UNTIL then
            local sub = elapsed % 60
            if sub >= 30 and sub < 34 then probe.pad_force(probe.BTN.CROSS) end
        end

        if not injected and elapsed >= INJECT_AT then
            injected = true
            probe.write_u8(me + 0x1DE, 2)
            probe.write_u8(me + 0x1DF, SPELL)
            probe.write_u8(me + 0x1DD, TARGET_SEAT)
            PCSX.log(string.format(
                "[inject t%d] seat%d +0x1DE=2 +0x1DF=0x%02X +0x1DD=%d char_kind=%d",
                elapsed, CASTER_SEAT, SPELL, TARGET_SEAT, u8(CHAR_KIND + CASTER_SEAT)))
        end

        if last_clip_vsync >= 0 and quit_at < 0 and elapsed > last_clip_vsync + TAIL then
            quit_at = elapsed
        end
        if quit_at >= 0 and elapsed >= quit_at then c.request_quit = true end
    end,

    on_done = function()
        PCSX.log(string.format("[cast-voice] cues=%d clips=%d resolves=%d",
            n_cues, n_clips, n_resolve))
    end,
})
