// @ts-check
// Who flies in a slot, from a tap on the name in its card: a list of the
// pilots (with many: search and a filter "Alle / Ohne Slot / Heute dabei"), "Kein Pilot" (the slot is free: not in the session),
// or a guest's name typed in. A tap takes effect at once and closes the
// dialog; a pilot from another slot swaps with this slot's pilot (presets
// go with them). A pilot with several presets stays open to choose one;
// the server gives a pilot with exactly one preset that one.

import { useState } from 'react'
import { CheckIcon, MagnifyingGlassIcon, ProhibitIcon } from '@phosphor-icons/react'
import { saveSettings } from '../api.js'
import { secs, slotColor } from '../format.js'
import { presetSummary } from '../model.js'
import Dialog from './Dialog.jsx'

/** From this many pilots on, the list gets a search field and the filter. */
const SEARCH_FROM = 6

const FILTERS = /** @type {const} */ ([
  ['all', 'Alle'],
  ['free', 'Ohne Slot'],
  ['today', 'Heute dabei'],
])

/**
 * @param {object} props
 * @param {import('../types').State} props.state
 * @param {import('../types').Stats | null} props.stats
 * @param {number} props.index
 * @param {() => void} props.onClose
 */
export default function PilotDialog({ state, stats, index, onClose }) {
  const s = state.settings
  const seats = s.channels.map((_, i) => s.seats[i] ?? { pilot_id: null, label: '', enabled: true })
  const seat = seats[index]
  const ch = s.channels[index]
  const running = state.active != null
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState(/** @type {(typeof FILTERS)[number][0]} */ ('all'))
  const [guest, setGuest] = useState(seat.enabled && !seat.pilot_id ? seat.label : '')
  const [error, setError] = useState('')

  const current = seat.enabled && !seat.label.trim() ? seat.pilot_id : null
  const free = !seat.enabled

  /** @param {import('../types').SeatSetting[]} next @param {boolean} close */
  async function save(next, close) {
    setError('')
    try {
      await saveSettings(s, { seats: next })
      if (close) onClose()
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    }
  }

  /** Pilot for this slot; the slot that had them gets this slot's pilot.
   * @param {import('../types').Pilot} p */
  function pick(p) {
    const presets = state.presets.filter((x) => x.pilot_id === p.id)
    if (p.id === current) return presets.length > 1 ? undefined : onClose()
    const next = seats.map((x) => ({ ...x }))
    const other = next.findIndex((x, j) => j !== index && x.pilot_id === p.id)
    const preset = other >= 0 ? next[other].preset_id : null
    if (other >= 0) next[other] = { ...next[other], pilot_id: seat.pilot_id, preset_id: seat.preset_id }
    next[index] = { ...next[index], pilot_id: p.id, preset_id: preset, label: '', enabled: true }
    save(next, presets.length <= 1)
  }
  const pickNone = () => save(seats.map((x, j) => (j === index ? { ...x, pilot_id: null, preset_id: null, label: '', enabled: false } : x)), true)
  function pickGuest() {
    const name = guest.trim()
    if (!name) return
    save(seats.map((x, j) => (j === index ? { ...x, pilot_id: null, preset_id: null, label: name, enabled: true } : x)), true)
  }
  /** @param {number | null} id */
  const pickPreset = (id) => save(seats.map((x, j) => (j === index ? { ...x, preset_id: id } : x)), true)

  const pb = (/** @type {number} */ id) => stats?.all.find((x) => x.pilot_id === id)?.best_lap_us
  /** The slot pilot `id` flies in (not this one), or -1. @param {number} id */
  const slotOf = (id) => seats.findIndex((x, j) => j !== index && x.enabled && !x.label.trim() && x.pilot_id === id)
  const today = new Set(stats?.today.map((x) => x.pilot_id) ?? [])
  /** @type {Record<(typeof FILTERS)[number][0], (p: import('../types').Pilot) => boolean>} */
  const keep = {
    all: () => true,
    free: (p) => p.id === current || slotOf(p.id) < 0,
    today: (p) => p.id === current || today.has(p.id),
  }
  const q = query.trim().toLowerCase()
  const found = state.pilots.filter((p) => !q || `${p.name} ${p.full_name}`.toLowerCase().includes(q))
  const pilots = found.filter(keep[filter]).sort((a, b) => a.name.localeCompare(b.name, 'de'))

  return (
    <Dialog
      onClose={onClose}
      title={
        <span className="dialog-title">
          <span className="dot" style={{ background: slotColor(index) }} />
          <span>
            <span className="muted small tnum">
              Slot {index + 1} · {ch.name} · {ch.mhz} MHz
            </span>
            <span>Wer fliegt?</span>
          </span>
        </span>
      }
    >
      {running && <p className="muted small" style={{ margin: 0 }}>Es läuft eine Session: Änderungen gelten ab dem nächsten Start.</p>}
      {state.pilots.length >= SEARCH_FROM && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <label className="search">
            <MagnifyingGlassIcon size={16} />
            <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Pilot suchen" aria-label="Pilot suchen" />
          </label>
          <div className="seg">
            {FILTERS.map(([k, label]) => (
              <button key={k} className={filter === k ? 'on' : ''} onClick={() => setFilter(k)}>
                {label} <span className="tnum" style={{ opacity: 0.7 }}>{found.filter(keep[k]).length}</span>
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="pick-list">
        <button className={'pick-row' + (free ? ' on' : '')} onClick={pickNone}>
          <span className="avatar-sm none">
            <ProhibitIcon size={16} />
          </span>
          <span className="text">
            <span className="pick-name">Kein Pilot</span>
            <span className="hint">Slot frei: zählt nicht für Rangliste und Ansagen</span>
          </span>
          {free && <CheckIcon size={18} className="check" />}
        </button>

        <span className="kicker" style={{ padding: '12px 10px 4px' }}>
          Piloten · {pilots.length}
        </span>
        {pilots.map((p) => {
          const on = p.id === current
          const elsewhere = slotOf(p.id)
          const presets = state.presets.filter((x) => x.pilot_id === p.id)
          const best = pb(p.id)
          return (
            <div key={p.id} className={'pick-item' + (on ? ' on' : '')}>
              <button className="pick-row" onClick={() => pick(p)}>
                <span className="avatar-sm" style={{ boxShadow: `0 0 0 2px ${elsewhere >= 0 ? slotColor(elsewhere) : on ? slotColor(index) : 'var(--n800)'}` }}>
                  {initials(p.full_name || p.name)}
                </span>
                <span className="text">
                  <span className="pick-name">{p.name}</span>
                  <span className="hint">{[p.full_name, best ? `PB ${secs(best)}` : ''].filter(Boolean).join(' · ') || 'neu'}</span>
                </span>
                {elsewhere >= 0 && (
                  <span className="slot-pill" style={{ color: slotColor(elsewhere), borderColor: slotColor(elsewhere) }}>
                    Slot {elsewhere + 1} · tauschen
                  </span>
                )}
                {on && <CheckIcon size={18} className="check" />}
              </button>
              {on && presets.length > 1 && (
                <div className="pick-presets">
                  <span className="hint">Welche Drohne? (Preset der Erkennung)</span>
                  <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
                    {presets.map((r) => (
                      <button key={r.id} className={'chip-btn' + (seat.preset_id === r.id ? ' on' : '')} onClick={() => pickPreset(r.id)} title={presetSummary(r)}>
                        {r.name}
                      </button>
                    ))}
                    <button className={'chip-btn' + (seat.preset_id == null ? ' on' : '')} onClick={() => pickPreset(null)} title="Kanal behält seine Werte">
                      Keins
                    </button>
                  </div>
                </div>
              )}
            </div>
          )
        })}
        {!state.pilots.length && <span className="muted small" style={{ padding: '8px 10px' }}>Noch keine Piloten: unter „Piloten“ anlegen, oder unten als Gast eintragen.</span>}
        {state.pilots.length > 0 && !pilots.length && (
          <span className="muted small" style={{ padding: '8px 10px' }}>
            Kein Pilot passt{filter !== 'all' ? ' (Filter „' + FILTERS.find(([k]) => k === filter)?.[1] + '“)' : ''}. Unten als Gast eintragen.
          </span>
        )}
      </div>

      <div className="field" style={{ borderTop: '1px solid var(--n900)', paddingTop: 12 }}>
        <span>Gast ohne Profil</span>
        <div style={{ display: 'flex', gap: 8 }}>
          <input
            className="input"
            value={guest}
            placeholder="Name"
            onChange={(e) => setGuest(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && pickGuest()}
            aria-label="Name des Gasts"
          />
          <button className="btn btn-secondary" disabled={!guest.trim()} onClick={pickGuest}>
            Eintragen
          </button>
        </div>
      </div>
      {error && <p className="error">{error}</p>}
    </Dialog>
  )
}

/** "Kai Berger" → "KB", "NITRO" → "NI". @param {string} name */
const initials = (name) => {
  const words = name.trim().split(/\s+/)
  return (words.length > 1 ? words.map((w) => w[0]).join('') : name.slice(0, 2)).slice(0, 2).toUpperCase()
}
