-- autorun_keyon_census.lua
--
-- Counts retail KEY-ONS per emulated vsync straight off the CPU, with no SPU
-- state involved.
--
-- Why: the per-vsync SPU capture (`autorun_audio_trace.lua`) can only see a
-- key-on as a voice's envelope rising from zero. That edge is measured on
-- PCSX-Redux's SPU thread, which runs on the host's audio clock rather than
-- the emulated one (docs/subsystems/audio.md, "The envelope channel is not on
-- emulated time"): a short note can attack and drain between two captures and
-- never be seen, and a note re-keyed while the voice still rings is no edge at
-- all. A key-on itself is a call the score makes from the game's own vsync
-- handler, so counting the call is exact on both sides.
--
-- Hook: `FUN_8006B854(mode, mask24)` (libspu `SpuSetKey`) - the per-frame flush
-- `FUN_80065BAC` calls it with `a0 = 1` and the staged key-on accumulator
-- (`_DAT_801CDB48/4A`) as `a1` at `0x80065F74`, and with `a0 = 0` / the
-- key-off accumulator at `0x80065F54`. One row per call.
--
-- Output CSV: vsync,mode,mask,ra,records
--   vsync    post-load vsync index (the probe's elapsed counter)
--   mode     1 = KON, 0 = KOFF
--   mask     24-bit voice mask (hex)
--   ra       caller
--   records  for KON rows: `voice:tone_page/tone/vab_slot/owner` per keyed
--            voice, read off the libsnd note record at 0x801CDB50 + v*0x36
--            (owner 0021 = an SFX cue, anything else a sequence)
-- plus three other row kinds in the `mode` column:
--   note     one FUN_80066308 call: `mask` = owner key, records =
--            vab/prog/key/vel - the score's note stream before allocation
--   alloc    the allocator's verdict (winning voice, voice count, request
--            priority; `DROP` when every voice outranks the note)
--   clock    every 60 vsyncs: wall-clock ms, the open sequences' channel
--            mute masks, the resolved PROT index, pool base and BGM id
-- and the SFX side, which the engine trace's `sfx_ring` ids answer:
--   cuekey   FUN_80065034, the cue key-on (`mask` = voice, records =
--            VAB slot / program / tone); `ra` 0x80016D9C = the ring drainer
--   cue / cuerepl   FUN_80035B50 / FUN_80035BD0 ring push / replace (id)
--   ringw    any store into the ring ids `DAT_8007B6D8[4]` (`mask` = pc);
--            pc 0x80016B28 is the ager clearing a played slot, 0x80023688
--            the move VM's op 0x1D
--   motioncue  motion-VM op 0x09 pushes, with the actor and bytecode pc
--   xa       FUN_8003D53C CD-XA one-shot clips (slot, channel, duration)
--   spawn    FUN_80021B04 effect-part spawns (LEGAIA_CENSUS_SPAWNS=1)
--
-- Run (no save-state dumps per frame, so the emulator stays near real time):
--   LEGAIA_FRAMES=4000 bash scripts/pcsx-redux/run_probe.sh \
--       --scenario s3_rimelm_freeroam \
--       --lua scripts/pcsx-redux/autorun_keyon_census.lua --out <path>.csv

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local ffi   = require("ffi")

ffi.cdef [[
typedef long __kc_time_t;
struct __kc_timespec { __kc_time_t tv_sec; long tv_nsec; };
int clock_gettime(int clk_id, struct __kc_timespec *tp);
]]
local ts = ffi.new("struct __kc_timespec[1]")
local function now_ms()
    ffi.C.clock_gettime(1, ts)
    return tonumber(ts[0].tv_sec) * 1000 + tonumber(ts[0].tv_nsec) / 1e6
end

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 600)
local BOOT_DELAY = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local OUT_PATH   = probe.out_path("keyon_census.csv")

local SPU_SET_KEY = 0x8006B854
local NOTE_REC    = 0x801CDB50
local NOTE_STRIDE = 0x36

local function n32(v) return (tonumber(v) or 0) % 0x100000000 end

local csv = probe.csv_open(OUT_PATH, "vsync,mode,mask,ra,records")
local vsync = -1
local hits = 0

local function records(mask)
    local parts = {}
    for v = 0, 23 do
        if bit.band(mask, bit.lshift(1, v)) ~= 0 then
            local base = NOTE_REC + v * NOTE_STRIDE
            parts[#parts + 1] = string.format("%d:%d/%d/%d/%04X", v,
                probe.read_u16(base + 0x12), probe.read_u16(base + 0x16),
                probe.read_u16(base + 0x18), probe.read_u16(base + 0x10))
        end
    end
    return table.concat(parts, " ")
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    boot_delay     = BOOT_DELAY,
    snapshot_every = 1000000,

    on_arm = function()
        probe.arm_breakpoint(SPU_SET_KEY, "Exec", 4, "spu_set_key", function()
            local r = PCSX.getRegisters()
            local mode = n32(r.GPR.n.a0)
            local mask = bit.band(n32(r.GPR.n.a1), 0xFFFFFF)
            if mask == 0 then return end
            hits = hits + 1
            local rec = (mode == 1) and records(mask) or ""
            if csv then
                pcall(function()
                    csv:row("%d,%d,%06X,%08X,%s", vsync, mode, mask,
                        n32(r.GPR.n.ra), rec)
                end)
            end
        end)
        -- Every note the score asks for, before any voice is chosen:
        -- `FUN_80066308(owner, vab, prog, note, vel, ...)` - `a0` is the
        -- `seq | track << 8` owner key (`0x21` = an SFX cue), `a3` the key
        -- staged at `0x801CE34A`, the fifth argument the velocity (a zero
        -- velocity is routed to the note-off `FUN_8006688C`).
        probe.arm_breakpoint(0x80066308, "Exec", 4, "note_on", function()
            local r = PCSX.getRegisters()
            local sp = n32(r.GPR.n.sp)
            if csv then
                pcall(function()
                    csv:row("%d,note,%04X,%08X,vab=%d prog=%d key=%d vel=%d",
                        vsync, bit.band(n32(r.GPR.n.a0), 0xFFFF), n32(r.GPR.n.ra),
                        bit.band(n32(r.GPR.n.a1), 0xFFFF),
                        bit.band(n32(r.GPR.n.a2), 0xFFFF),
                        bit.band(n32(r.GPR.n.a3), 0xFF),
                        bit.band(probe.read_u32(sp + 0x10), 0xFFFF))
                end)
            end
        end)
        -- Every cue pushed onto the SFX ring: `FUN_80035B50(id)` (the field
        -- and battle producers' ring push) and `FUN_80035BD0(id)` (overwrite
        -- the last slot). `mask` carries the cue id.
        -- `FUN_80065034` is the direct cue key-on (owner 0x21) every cue path
        -- ends in; its `ra` names the producer that skipped the ring.
        probe.arm_breakpoint(0x80065034, "Exec", 4, "cue_keyon", function()
            local r = PCSX.getRegisters()
            if csv then
                pcall(function()
                    csv:row("%d,cuekey,%04X,%08X,a1=%X a2=%X a3=%X", vsync,
                        bit.band(n32(r.GPR.n.a0), 0xFFFF), n32(r.GPR.n.ra),
                        n32(r.GPR.n.a1), n32(r.GPR.n.a2), n32(r.GPR.n.a3))
                end)
            end
        end)
        -- Any store into the four ring ids `DAT_8007B6D8[4]`: names the
        -- producers that write a slot without the push pair.
        probe.arm_breakpoint(0x8007B6D8, "Write", 8, "ring_write", function()
            local r = PCSX.getRegisters()
            if csv then
                pcall(function()
                    csv:row("%d,ringw,%08X,%08X,ids=%04X/%04X/%04X/%04X", vsync,
                        n32(r.pc), n32(r.GPR.n.ra),
                        probe.read_u16(0x8007B6D8), probe.read_u16(0x8007B6DA),
                        probe.read_u16(0x8007B6DC), probe.read_u16(0x8007B6DE))
                end)
            end
        end)
        -- Effect-part spawns (`FUN_80021B04`), opt-in: names who seats the
        -- ambient parts whose op-0x1D stores sound a scene.
        if os.getenv("LEGAIA_CENSUS_SPAWNS") then
            probe.arm_breakpoint(0x80021B04, "Exec", 4, "spawn", function()
                local r = PCSX.getRegisters()
                if csv then
                    pcall(function()
                        csv:row("%d,spawn,%08X,%08X,a1=%X a2=%X a3=%X", vsync,
                            n32(r.GPR.n.a0), n32(r.GPR.n.ra), n32(r.GPR.n.a1),
                            n32(r.GPR.n.a2), n32(r.GPR.n.a3))
                    end)
                end
            end)
        end
        -- Motion-VM cue pushes (`FUN_80038158` op 0x09, `jal 0x80035B50` at
        -- 0x80039178): `s3` is the actor, `s1` the op's bytecode; the row
        -- carries the actor's world position (+0x14/+0x18/+0x1C).
        probe.arm_breakpoint(0x80039178, "Exec", 4, "motion_cue", function()
            local r = PCSX.getRegisters()
            local a = n32(r.GPR.n.s3)
            if csv then
                pcall(function()
                    local function s16(x)
                        local v = probe.read_u16(x)
                        return v >= 0x8000 and v - 0x10000 or v
                    end
                    csv:row("%d,motioncue,%08X,%08X,pos=%d/%d/%d pc=%08X", vsync, a,
                        n32(r.GPR.n.ra), s16(a + 0x14), s16(a + 0x18), s16(a + 0x1C),
                        n32(r.GPR.n.s1))
                end)
            end
        end)
        -- CD-XA one-shot clips: `FUN_8003D53C(clip_slot, chan, dur)`.
        probe.arm_breakpoint(0x8003D53C, "Exec", 4, "xa_clip", function()
            local r = PCSX.getRegisters()
            if csv then
                pcall(function()
                    csv:row("%d,xa,%02X,%08X,chan=%d dur=%d", vsync,
                        bit.band(n32(r.GPR.n.a0), 0xFF), n32(r.GPR.n.ra),
                        bit.band(n32(r.GPR.n.a1), 0xFF),
                        bit.band(n32(r.GPR.n.a2), 0xFFFF))
                end)
            end
        end)
        for _, hook in ipairs({ { 0x80035B50, "cue" }, { 0x80035BD0, "cuerepl" } }) do
            probe.arm_breakpoint(hook[1], "Exec", 4, hook[2], function()
                local r = PCSX.getRegisters()
                if csv then
                    pcall(function()
                        csv:row("%d,%s,%04X,%08X,", vsync, hook[2],
                            bit.band(n32(r.GPR.n.a0), 0xFFFF), n32(r.GPR.n.ra))
                    end)
                end
            end)
        end
        -- Voice allocator verdict: at 0x80066C84 `s0` holds the winning
        -- voice, or the voice count `_DAT_801CE344` when every voice
        -- outranks the request and the note is DROPPED (FUN_80066B00).
        probe.arm_breakpoint(0x80066C84, "Exec", 4, "voice_alloc", function()
            local r = PCSX.getRegisters()
            local win = bit.band(n32(r.GPR.n.s0), 0xFF)
            local vmax = probe.read_u8(0x801CE344)
            local prior = probe.read_u8(0x801CE357)
            if csv then
                pcall(function()
                    csv:row("%d,alloc,%02X,%08X,win=%d vmax=%d prior=%d%s",
                        vsync, win, n32(r.GPR.n.ra), win, vmax, prior,
                        (win >= vmax) and " DROP" or "")
                end)
            end
        end)
        PCSX.log(string.format("[keyon] armed SpuSetKey hook for %d vsyncs -> %s",
            FRAMES, OUT_PATH))
        return {}
    end,

    on_capture = function(ctx, elapsed)
        vsync = elapsed
        -- Wall-clock pace, every 60 vsyncs: the libsnd allocator reads each
        -- voice's envelope back off the SPU, and PCSX-Redux runs that
        -- envelope on the host's audio clock, so a run far below real time
        -- allocates (and drops) differently from the hardware.
        if elapsed % 60 == 0 and csv then
            -- `_DAT_8007BAC8` is the BGM id the state has loaded - the
            -- track the engine side must play for the census to compare;
            -- `0x8007BA9C` the PROT index it resolved to, `0x8007BC64` the
            -- global pool's base.
            -- `mute=` is the per-channel note mask at `+0x80` of each open
            -- sequence's track-0 record (`*(0x801CD2C0 + seq*4)`, stride
            -- 0xB0): FUN_80061B24 drops a note-on whose channel bit is set.
            pcall(function()
                local mutes = {}
                for s = 0, 3 do
                    local p = n32(probe.read_u32(0x801CD2C0 + s * 4))
                    if probe.in_ram(p) then
                        mutes[#mutes + 1] = string.format("%d:%04X", s,
                            probe.read_u16(p + 0x80))
                    end
                end
                csv:row("%d,clock,0,0,ms=%.1f mute=%s prot=%d pool=%d bgm=%d",
                    elapsed, now_ms(), table.concat(mutes, "/"),
                    probe.read_u32(0x8007BA9C),
                    probe.read_u32(0x8007BC64), probe.read_u16(0x8007BAC8))
            end)
        end
        if elapsed >= FRAMES then ctx.request_quit = true end
    end,

    on_done = function()
        PCSX.log(string.format("[keyon] %d non-empty SpuSetKey call(s)", hits))
    end,
})
