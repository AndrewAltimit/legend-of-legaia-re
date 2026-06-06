-- autorun_party_attack_damage_trace.lua
--
-- Decide whether a PARTY member's basic "Attack" routes its damage through the
-- move-power damage kernel FUN_801dd0ac (the 0x801F4F5C move-power-table path the
-- enemy special-attacks use), or through a different damage function.
--
-- Method: watch every live battle actor's HP word (+0x14c) for WRITES. When an
-- attack lands, the watchpoint fires AT the store instruction, so r.pc names the
-- function that applied the damage - no assumption about which function that is.
-- A second Exec breakpoint on FUN_801dd0ac confirms when the move-power kernel
-- itself runs. Cross-referencing the two answers the question:
--   - enemy attack on a party member -> writer pc should be the move-power path
--     (already known/wired);
--   - Vahn's basic Attack on the enemy -> if the writer pc is the SAME path,
--     party basic attacks use the move-power table; if it's a different fn, they
--     don't.
--
-- Run against the party-basic-attack battle save (Vahn with a queued basic
-- Attack on a lone Gobu Gobu). Drive the turn yourself (or let the probe tap
-- X / Up): the probe captures every HP write with its writer pc.
--
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_party_attack_damage_trace.lua \
--     --sstate ~/Tools/pcsx-redux/SCUS94254.sstate8 --frames 1200

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate8")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1200)
-- Set LEGAIA_DRIVE=0 to disable scripted input (drive the fight by hand).
local DRIVE = probe.getenv_num("LEGAIA_DRIVE", 1)

local DAMAGE_KERNEL = 0x801DD0AC -- FUN_801dd0ac: arts/physical predamage
local CTX_PTR       = 0x8007BD24 -- _DAT_8007bd24 -> battle ctx
local GMODE         = 0x8007B83C -- _DAT_8007b83c game mode (battle = 0x15)
local HP_OFF        = 0x14C       -- actor current HP (u16)
local MOVEID_OFF    = 0x1DF       -- actor queued move id (u8)
local SCALE_OFF     = 0x72        -- actor render scale (~0x1000) - actor signature

local function s16(v) if v >= 0x8000 then return v - 0x10000 end return v end
-- Coerce a possibly-signed 32-bit value to an unsigned Lua number (LuaJIT
-- bit ops return signed 32-bit, which breaks address arithmetic). read_u32
-- already returns unsigned, but register reads / bit ops need this.
local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
local function u32(addr) return probe.read_u32(addr) or 0 end
local function ctx() return u32(CTX_PTR) end

-- Find live battle-state actors via the battle-action actor-pointer array
-- (&DAT_801c9370)[slot] - the table the battle code itself indexes. A populated
-- combat actor is a pointer into the battle heap (>= 0x800E0000) with a
-- plausible HP (1..9999) at +0x14c and a move id < 0x80 at +0x1df. (The static
-- stat templates at arr[8..] live at 0x80078xxx and are excluded by the heap
-- range.) Slots 0..2 are the party, 3.. are enemies.
local ACTOR_HEAP_LO = 0x800E0000
local function find_actors()
    local out = {}
    for slot = 0, 11 do
        local ap = u32(ACTOR_ARRAY + slot * 4)
        if ap >= ACTOR_HEAP_LO and ap < 0x80200000 then
            local hp = s16(probe.read_u16(ap + HP_OFF) or 0)
            local mid = probe.read_u8(ap + MOVEID_OFF) or 0
            if hp >= 1 and hp <= 9999 and mid < 0x80 then
                out[#out + 1] = { ptr = ap, slot = slot }
            end
        end
    end
    return out
end

local function actor_str(e)
    return string.format("slot%d=0x%08X[mid=0x%02X hp=%d]", e.slot, e.ptr,
        probe.read_u8(e.ptr + MOVEID_OFF) or 0, s16(probe.read_u16(e.ptr + HP_OFF) or 0))
end
local function dump_actors(tag)
    local a = find_actors()
    local p = {}
    for _, e in ipairs(a) do p[#p + 1] = actor_str(e) end
    PCSX.log(string.format("[actors %s] gm=0x%02X ctx=0x%08X n=%d  %s",
        tag, probe.read_u8(GMODE) or 0, ctx(), #a, table.concat(p, "  ")))
end

-- Is a pc inside the move-power damage kernel's body (FUN_801dd0ac, ~0x400 B)?
local function in_move_power_path(pc)
    return pc >= 0x801DD0AC and pc < 0x801DD4AC
end

local hp_watch_armed = false
local hp_last = {}        -- ptr -> last seen HP
local kernel_hits = 0
local got_decrease = 0

local function arm_hp_watches()
    local actors = find_actors()
    if #actors < 1 then return false end
    for _, e in ipairs(actors) do
        local who, slot = e.ptr, e.slot
        hp_last[who] = s16(probe.read_u16(who + HP_OFF) or 0)
        probe.arm_breakpoint(who + HP_OFF, "Write", 2, "hp_" .. string.format("%08X", who),
            function()
                local r = PCSX.getRegisters()
                local pc = tou32(r.pc)
                local newhp = s16(probe.read_u16(who + HP_OFF) or 0)
                local old = hp_last[who] or newhp
                local delta = newhp - old
                hp_last[who] = newhp
                -- Only a plausible combat HP change is a real hit/heal.
                if newhp < 0 or newhp > 9999 then return end
                if delta < 0 then got_decrease = got_decrease + 1 end
                PCSX.log(string.format(
                    "[hpwrite] slot%d actor=0x%08X (mid=0x%02X) hp %d->%d (delta=%d)  writer_pc=0x%08X  %s",
                    slot, who, probe.read_u8(who + MOVEID_OFF) or 0, old, newhp, delta, pc,
                    in_move_power_path(pc) and "<== MOVE-POWER KERNEL PATH" or "(other damage fn)"))
            end)
    end
    local slots = {}
    for _, e in ipairs(actors) do slots[#slots + 1] = tostring(e.slot) end
    PCSX.log(string.format("[probe] armed HP write-watch on %d actors (slots %s)",
        #actors, table.concat(slots, ",")))
    return true
end

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function(c)
        PCSX.log("== party attack damage trace (HP-write watch) ==")
        dump_actors("arm")
        probe.arm_breakpoint(DAMAGE_KERNEL, "Exec", 4, "damage_kernel", function()
            local r = PCSX.getRegisters()
            local a1 = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFF)
            kernel_hits = kernel_hits + 1
            PCSX.log(string.format("[dmg-kernel #%d] FUN_801dd0ac a0=0x%02X a1(slot)=%d",
                kernel_hits, bit.band(tonumber(r.GPR.n.a0) or 0, 0xFF), a1))
        end)
        return { { addr = DAMAGE_KERNEL, name = "damage_kernel" } }
    end,
    on_capture = function(c, elapsed)
        -- Arm the HP watches as soon as the battle actors are live (the save
        -- boots into the fight, so they appear a few frames in).
        if not hp_watch_armed and probe.read_u8(GMODE) == 0x15 then
            if arm_hp_watches() then hp_watch_armed = true end
        end

        if DRIVE ~= 0 then
            -- Robust input: force the button every frame across a window. Tap X
            -- (Begin) once the menu is up, then drive Vahn's attack with Up.
            if elapsed >= 20 and elapsed <= 34 then
                probe.pad_force(probe.BTN.CROSS)
            elseif elapsed == 36 then
                probe.pad_release(probe.BTN.CROSS)
            elseif elapsed >= 120 and elapsed <= 600 then
                if (elapsed % 30) < 14 then probe.pad_force(probe.BTN.UP)
                else probe.pad_release(probe.BTN.UP) end
            elseif elapsed == 602 then
                probe.pad_release(probe.BTN.UP)
            end
        end

        if elapsed % 60 == 0 then
            dump_actors("t" .. elapsed)
            PCSX.log(string.format("[diag t%d] kernel_hits=%d hp_decreases=%d armed=%s",
                elapsed, kernel_hits, got_decrease, tostring(hp_watch_armed)))
        end
        -- Safety stop: once we've captured plenty of damaging HP writes (both the
        -- enemy turn and Vahn's attack), end. Otherwise the window/timeout bounds it.
        if got_decrease >= 10 and elapsed > 30 then c.request_quit = true end
    end,
})
