-- midi_encoder.lua
--
-- Battle-diorama MIDI transport encoder. Turns a BattleState table (the output
-- of probe.battle_state.read()) into a list of MIDI Control-Change messages per
-- the register protocol in register_schema.toml (via the generated registers
-- table). Pure + stateful-by-instance: it tracks the last value sent on each
-- (channel, cc) so `delta()` emits only what changed; it reads no memory and has
-- no PCSX dependency, so it unit-tests offline against synthetic BattleStates.
--
-- This is a TRANSPORT layer on top of the shared extraction (PRD sec 11): the
-- register map here is a strict subset of the spectator viewport's BattleState
-- (ids + scalars, no transforms). The encoder emits messages; a sink (virtual
-- MIDI port, log, or the pixel-strip fallback) sends them.
--
-- Usage:
--   local enc = require("midi_encoder")
--   local e = enc.new()                 -- optionally enc.new({ registers = T })
--   e:tick()                            -- advance heartbeat once per vsync
--   local msgs = e:full(state)          -- full sweep: all registers + commits
--   local msgs = e:delta(state)         -- only changed registers + commits
--   for _, m in ipairs(msgs) do send(enc.pack(m)) end   -- m = {ch, cc, val}

local M = {}

-- The generated register map. Injectable for tests; defaults to require.
local function default_registers()
    return require("registers")
end

local function clamp7(v) if v < 0 then return 0 elseif v > 127 then return 127 end return v end
local function clamp14(v) if v < 0 then return 0 elseif v > 16383 then return 16383 end return v end
local function hi7(v) return math.floor(v / 128) % 128 end
local function lo7(v) return v % 128 end
local function band(a, b) return bit and bit.band(a, b) or (a % (2 * b) >= b and b or 0) end

-- A MIDI CC message packed to three status/data bytes (status = 0xB0 | channel).
function M.pack(m)
    return string.char(0xB0 + (m.ch % 16), m.cc % 128, m.val % 128)
end

function M.to_bytes(msgs)
    local parts = {}
    for i = 1, #msgs do parts[i] = M.pack(msgs[i]) end
    return table.concat(parts)
end

function M.describe(m)
    return string.format("ch%-2d cc0x%02X = %3d", m.ch, m.cc, m.val)
end

local Encoder = {}
Encoder.__index = Encoder

function M.new(opts)
    opts = opts or {}
    local reg = opts.registers or default_registers()
    return setmetatable({
        reg = reg,
        last = {},            -- [ch] = { [cc] = val } last sent
        heartbeat = 0,
        refresh_marker = 0,
    }, Encoder)
end

function Encoder:reset()
    self.last = {}
end

-- Advance the heartbeat counter (call once per polled vsync). Drives the
-- decoder's LIVE/STALE indicator.
function Encoder:tick()
    self.heartbeat = (self.heartbeat + 1) % 128
end

-- Map an actor status word (+0x00) to the 7-bit status register. Only the
-- confuse/charm bit (word bit 0x2000) is pinned; the rest is reserved.
local function status_mask(reg, word)
    local m = 0
    if word and word % (0x4000) >= 0x2000 then
        m = m + (2 ^ reg.STATUS_BITS.confuse)
    end
    return clamp7(m)
end

local function derive_phase(reg, state)
    if not state.in_battle then return reg.BATTLE_PHASE.none end
    local enemies, enemies_alive = 0, 0
    local party, party_alive = 0, 0
    for slot = 0, 7 do
        local s = state.slots[slot]
        if s and s.present then
            if s.role == "enemy" then
                enemies = enemies + 1
                if s.hp > 0 then enemies_alive = enemies_alive + 1 end
            else
                party = party + 1
                if s.hp > 0 then party_alive = party_alive + 1 end
            end
        end
    end
    if party > 0 and party_alive == 0 then return reg.BATTLE_PHASE.defeat end
    if enemies > 0 and enemies_alive == 0 then return reg.BATTLE_PHASE.victory end
    return reg.BATTLE_PHASE.active
end

-- Desired flat cc->value map for the meta channel.
function Encoder:meta_values(state)
    local reg = self.reg
    local v = {}
    local m = reg.META
    v[m.schema_version.cc] = reg.SCHEMA_VERSION
    v[m.session_flags.cc]  = clamp7(state.session_flags or 0)
    v[m.heartbeat.cc]      = self.heartbeat % 128
    v[m.game_mode.cc]      = clamp7(state.render_mode or 0)
    v[m.battle_phase.cc]   = derive_phase(reg, state)
    v[m.refresh_marker.cc] = self.refresh_marker % 128
    local region = clamp14(state.region_id or 0)
    v[m.region_id.hi] = hi7(region)
    v[m.region_id.lo] = lo7(region)
    return v
end

-- Desired flat cc->value map for one actor-slot channel.
function Encoder:slot_values(state, slot)
    local reg = self.reg
    local sl = reg.SLOT
    local s = state.slots[slot]
    local present = s and s.present or false
    local v = {}

    local flags = 0
    if present then
        flags = flags + (2 ^ reg.FLAG_BITS.present)
        if s.hp <= 0 then flags = flags + (2 ^ reg.FLAG_BITS.dead) end
        if state.active_slot == slot then flags = flags + (2 ^ reg.FLAG_BITS.acting) end
        -- targeted bit: no per-actor target tracking yet (TODO target_slot).
    end
    v[sl.flags.cc] = clamp7(flags)

    -- id: enemy = monster id; party = character id (not yet read -> 0).
    local id = (present and s.role == "enemy") and s.monster_id or 0
    v[sl.id.hi] = hi7(clamp14(id))
    v[sl.id.lo] = lo7(clamp14(id))

    v[sl.action_id.cc] = present and clamp7(s.action or 0) or 0

    local hp = present and clamp14(math.max(0, s.hp)) or 0
    v[sl.hp.hi] = hi7(hp); v[sl.hp.lo] = lo7(hp)
    local mhp = present and clamp14(math.max(0, s.hp_max)) or 0
    v[sl.maxhp.hi] = hi7(mhp); v[sl.maxhp.lo] = lo7(mhp)

    v[sl.status.cc] = present and status_mask(reg, s.status) or 0
    v[sl.target_slot.cc] = reg.CONSTANTS.target_none
    return v
end

-- All channels, in emission order (meta first, then slots 0..7).
function Encoder:all_channels()
    local chans = { self.reg.META_CHANNEL }
    for _, c in ipairs(self.reg.PARTY_CHANNELS) do chans[#chans + 1] = c end
    for _, c in ipairs(self.reg.ENEMY_CHANNELS) do chans[#chans + 1] = c end
    return chans
end

function Encoder:values_for(state, ch)
    if ch == self.reg.META_CHANNEL then return self:meta_values(state) end
    return self:slot_values(state, ch)
end

-- Emit one channel's registers into `msgs`. `force` = full sweep (emit every
-- register); otherwise emit only registers whose value changed since last send.
-- A commit (cc 0x7F) is appended iff at least one data register was emitted.
local COMMIT_CC = 0x7F
function Encoder:emit_channel(msgs, ch, values, force)
    local last = self.last[ch]
    if last == nil then last = {}; self.last[ch] = last end
    -- Ascending cc order puts wide hi before lo before commit automatically.
    local ccs = {}
    for cc in pairs(values) do ccs[#ccs + 1] = cc end
    table.sort(ccs)
    local changed = false
    for _, cc in ipairs(ccs) do
        local val = values[cc]
        if force or last[cc] ~= val then
            msgs[#msgs + 1] = { ch = ch, cc = cc, val = val }
            last[cc] = val
            changed = true
        end
    end
    if changed then
        msgs[#msgs + 1] = { ch = ch, cc = COMMIT_CC, val = 0 }
    end
end

-- Full sweep: every register on every channel + commits. Bumps refresh_marker
-- so a consumer can see the sweep boundary. Use on battle-enter, periodically,
-- and on (re)connect so a late/dropped consumer self-recovers.
function Encoder:full(state)
    self.refresh_marker = (self.refresh_marker + 1) % 128
    local msgs = {}
    for _, ch in ipairs(self:all_channels()) do
        self:emit_channel(msgs, ch, self:values_for(state, ch), true)
    end
    return msgs
end

-- Delta: only registers that changed since the previous full/delta, with a
-- commit on each channel that had any change. Empty when nothing changed.
function Encoder:delta(state)
    local msgs = {}
    for _, ch in ipairs(self:all_channels()) do
        self:emit_channel(msgs, ch, self:values_for(state, ch), false)
    end
    return msgs
end

return M
