// @ts-check
// Settings that rarely change: which channels are scanned, minimum lap,
// gain, the sound, the data (backups, deleting all sessions). (Boards and
// gates: their own page; slots, race format and manual thresholds: race
// control; exporting sessions: the sessions page.)

import { useEffect, useState } from 'react'
import { PlusIcon, PlayIcon, SpeakerHighIcon, SlidersIcon, TrashIcon, BroadcastIcon, CodeIcon, DatabaseIcon, DownloadSimpleIcon, ArchiveIcon } from '@phosphor-icons/react'
import { api, saveSettings } from '../api.js'
import { isSpeaker, setSpeaker } from '../announcer.js'
import { RACEBAND, dateTime, slotColor } from '../format.js'
import Dialog from '../components/Dialog.jsx'
import Stepper from '../components/Stepper.jsx'

/** @param {{state: import('../types').State, onOpenApi: () => void}} props */
export default function SettingsPage({ state, onOpenApi }) {
  const s = state.settings
  const running = state.active != null
  const [error, setError] = useState('')
  /** @param {Partial<import('../types').Settings>} patch */
  const save = (patch) => {
    setError('')
    return saveSettings(s, patch).catch((e) => setError(e.message))
  }

  function addChannel() {
    const used = new Set(s.channels.map((c) => c.mhz))
    const mhz = RACEBAND.find((f) => !used.has(f)) ?? 5800
    const name = RACEBAND.includes(mhz) ? `R${RACEBAND.indexOf(mhz) + 1}` : String(mhz)
    save({ channels: [...s.channels, { name, mhz }], seats: [...s.channels.map((_, i) => s.seats[i] ?? defSeat()), defSeat()] })
  }
  /** @param {number} i */
  const removeChannel = (i) =>
    save({ channels: s.channels.filter((_, j) => j !== i), seats: s.channels.map((_, j) => s.seats[j] ?? defSeat()).filter((_, j) => j !== i) })
  /** @param {number} i @param {string} value  "R3" or MHz */
  function setChannel(i, value) {
    const mhz = value.startsWith('R') ? RACEBAND[Number(value.slice(1)) - 1] : Number(value)
    if (!mhz || mhz < 5300 || mhz > 6000) return
    const name = RACEBAND.includes(mhz) ? `R${RACEBAND.indexOf(mhz) + 1}` : String(mhz)
    save({ channels: s.channels.map((c, j) => (j === i ? { name, mhz } : c)) })
  }

  return (
    <div className="two-col" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 420px), 1fr))' }}>
      <div className="panel">
        <div className="panel-head">
          <BroadcastIcon size={18} /> Kanäle
          <span className="end muted small">{s.channels.length} von 8</span>
        </div>
        <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <span className="muted small">
            Alle Boards scannen alle Kanäle. Weniger oder nah beieinander liegende Kanäle messen öfter. Wer auf welchem Kanal fliegt, stellst du in der
            Live-Ansicht am Zahnrad ein.
          </span>
          {running && <span className="error small">Während einer Session lassen sich Kanäle nicht ändern.</span>}
          {s.channels.map((c, i) => (
            <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span className="dot" style={{ background: slotColor(i) }} />
              <span style={{ width: 52 }}>Slot {i + 1}</span>
              <select
                className="input"
                style={{ width: 'auto', flex: 1 }}
                disabled={running}
                value={RACEBAND.includes(c.mhz) ? `R${RACEBAND.indexOf(c.mhz) + 1}` : 'x'}
                onChange={(e) => e.target.value !== 'x' && setChannel(i, e.target.value)}
              >
                {RACEBAND.map((f, k) => (
                  <option key={f} value={`R${k + 1}`}>
                    R{k + 1} · {f} MHz
                  </option>
                ))}
                {!RACEBAND.includes(c.mhz) && <option value="x">{c.mhz} MHz</option>}
              </select>
              <MhzInput value={c.mhz} disabled={running} onSet={(v) => setChannel(i, String(v))} />
              <button className="btn btn-ghost btn-icon" disabled={running || s.channels.length <= 1} onClick={() => removeChannel(i)} title="Kanal entfernen">
                <TrashIcon size={16} />
              </button>
            </div>
          ))}
          {s.channels.length < 8 && (
            <button className="btn btn-secondary" disabled={running} onClick={addChannel}>
              <PlusIcon size={16} /> Kanal hinzufügen
            </button>
          )}
        </div>
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
        <div className="panel">
          <div className="panel-head">
            <SlidersIcon size={18} /> Auswertung
          </div>
          <div className="panel-body" style={{ display: 'flex', flexWrap: 'wrap', gap: 20 }}>
            <Stepper label="Mindest-Rundenzeit" value={s.min_lap_s} unit="s" step={0.5} min={1} max={60} onSet={(v) => save({ min_lap_s: v })} />
            <Stepper label="Beste Runden in Folge" value={s.consecutive} step={1} min={2} max={20} onSet={(v) => save({ consecutive: v })} />
            <Stepper
              label="Verstärkung (Gain)"
              value={s.gain}
              step={1}
              min={0}
              max={60}
              hint="30 bei 25 mW; kleiner, wenn das Signal am Gate übersteuert"
              delayed
              onSet={(v) => save({ gain: v })}
            />
          </div>
        </div>
        <AnnouncePanel settings={s} save={save} />
        {error && <p className="error">{error}</p>}
        <DataPanel running={running} />
        {/* On phones the API page has no tab of its own. */}
        <button className="btn btn-secondary narrow-only" onClick={onOpenApi}>
          <CodeIcon size={16} /> API & Hooks
        </button>
      </div>
    </div>
  )
}

const LAP_CALLS = /** @type {const} */ ([
  ['off', 'Aus'],
  ['beep', 'Piep'],
  ['number', 'Runde'],
  ['time', 'Zeit'],
])
const LAP_HINTS = {
  off: 'Nichts',
  beep: 'Ein kurzer Ton',
  number: '„Nitro ist in Runde 3“',
  time: '„Nitro 13,7“ (Rundenzeit)',
}

/** Where announcements are played and which ones.
 * @param {{settings: import('../types').Settings, save: (patch: Partial<import('../types').Settings>) => void}} props */
function AnnouncePanel({ settings: s, save }) {
  const a = s.announce
  const [speaker, setSpeakerState] = useState(isSpeaker)
  /** @param {Partial<import('../types').AnnounceSettings>} patch */
  const set = (patch) => save({ announce: { ...a, ...patch } })
  /** @param {boolean} on */
  function toggleSpeaker(on) {
    setSpeaker(on)
    setSpeakerState(on)
  }
  /** @param {string} label @param {string} hint @param {keyof import('../types').AnnounceSettings} key */
  const toggle = (label, hint, key) => (
    <button className="row-toggle" disabled={!s.sound} onClick={() => set({ [key]: !a[key] })}>
      <span className="text">
        <span>{label}</span>
        <span className="hint">{hint}</span>
      </span>
      <span className={a[key] ? 'switch on' : 'switch'} />
    </button>
  )

  return (
    <div className="panel">
      <div className="panel-head">
        <SpeakerHighIcon size={18} /> Ansagen
        <button className="btn btn-ghost end" disabled={!s.sound} onClick={() => api('/announce/test', { method: 'POST' })} title="Beispiel-Ansage abspielen">
          <PlayIcon size={16} /> Probe
        </button>
      </div>
      <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        <button className="row-toggle" onClick={() => save({ sound: !s.sound })}>
          <span className="text">
            <span>Ansagen und Töne</span>
            <span className="hint">Alle an oder aus (auch oben rechts)</span>
          </span>
          <span className={s.sound ? 'switch on' : 'switch'} />
        </button>
        <div className="field">
          <span>Ausgabe</span>
          <div className="seg">
            <button className={a.output === 'server' ? 'on' : ''} onClick={() => set({ output: 'server' })}>
              Server
            </button>
            <button className={a.output === 'browser' ? 'on' : ''} onClick={() => set({ output: 'browser' })}>
              Browser (Laptop)
            </button>
          </div>
        </div>
        {a.output === 'browser' && (
          <button className="row-toggle" onClick={() => toggleSpeaker(!speaker)}>
            <span className="text">
              <span>Dieser Browser spielt die Ansagen</span>
              <span className="hint">
                Z. B. am Laptop mit der Bluetooth-Box. Nur Browser, in denen das an ist, spielen sie; die Seite muss dafür offen bleiben.
              </span>
            </span>
            <span className={speaker ? 'switch on' : 'switch'} />
          </button>
        )}
        {toggle('Start', '„Piloten, macht euch bereit“, „Rennen beginnt in“, 3 – 2 – 1, Startton; im Training „Training läuft“', 'start')}
        <div className="field" style={{ opacity: s.sound ? 1 : 0.45 }}>
          <span>Nach jeder Runde</span>
          <div className="seg">
            {LAP_CALLS.map(([k, label]) => (
              <button key={k} disabled={!s.sound} className={a.lap === k ? 'on' : ''} onClick={() => set({ lap: k })}>
                {label}
              </button>
            ))}
          </div>
          <span className="muted small">{LAP_HINTS[a.lap]}</span>
        </div>
        {toggle('Neue Bestzeit', 'Training: „Pilot Nitro hat eine neue Bestzeit erzielt: 13,7“ statt der Runden-Ansage', 'best')}
        {toggle('Pilot im Ziel', 'Rennen: „Nitro ist im Ziel“', 'finished')}
        {toggle('Rennen beendet', 'Rennen: wenn alle im Ziel sind', 'race_over')}
        <span className="muted small" style={{ marginTop: 8 }}>
          Fertige Sounds liegen im Ordner <code>sounds/</code>; Namen und Zeiten spricht die Sprachausgabe (auf dem Pi pico2wave oder espeak-ng).
        </span>
      </div>
    </div>
  )
}

const defSeat = () => ({ pilot_id: null, label: '', enabled: true })

/** Free frequency, saved on leaving the field. @param {{value: number, disabled: boolean, onSet: (v: number) => void}} props */
function MhzInput({ value, disabled, onSet }) {
  const [v, setV] = useState(String(value))
  useEffect(() => setV(String(value)), [value])
  return (
    <input
      className="input tnum"
      style={{ width: 84 }}
      inputMode="numeric"
      disabled={disabled}
      value={v}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => Number(v) !== value && onSet(Number(v))}
      title="Frequenz in MHz"
    />
  )
}

/** @typedef {{name: string, size: number, created_us: number}} Backup */

/** @param {number} n */
const size = (n) => (n < 1024 * 1024 ? `${Math.max(1, Math.round(n / 1024))} KB` : `${(n / 1024 / 1024).toFixed(1).replace('.', ',')} MB`)

/** Backups of the database and deleting all sessions (server/src/data.rs).
 * @param {{running: boolean}} props */
function DataPanel({ running }) {
  const [info, setInfo] = useState(/** @type {{dir: string, backups: Backup[]} | null} */ (null))
  const [busy, setBusy] = useState(false)
  const [msg, setMsg] = useState('')
  const [error, setError] = useState('')
  const [confirmReset, setConfirmReset] = useState(false)
  const [deleting, setDeleting] = useState(/** @type {string | null} */ (null))
  const load = () =>
    api('/backups')
      .then(setInfo)
      .catch((e) => setError(e.message))
  useEffect(() => {
    load()
  }, [])

  /** Runs a request; shows its error or `done(result)`. Returns whether it worked. @param {() => Promise<any>} req @param {(r: any) => string} done */
  async function run(req, done) {
    setBusy(true)
    setError('')
    setMsg('')
    try {
      setMsg(done(await req()))
      return true
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
      return false
    } finally {
      setBusy(false)
      load()
    }
  }
  const backup = () =>
    run(
      () => api('/backups', { method: 'POST' }),
      (r) => `Gespeichert: ${r.name}`,
    )
  const reset = () =>
    run(
      () => api('/reset', { method: 'POST' }),
      (r) => `${r.deleted} ${r.deleted === 1 ? 'Session' : 'Sessions'} gelöscht. Vorher gesichert: ${r.backup}`,
    ).then((ok) => ok && setConfirmReset(false))
  /** @param {string} name */
  const remove = (name) =>
    run(
      () => api(`/backups/${encodeURIComponent(name)}`, { method: 'DELETE' }),
      () => '',
    ).then(() => setDeleting(null))

  return (
    <div className="panel">
      <div className="panel-head">
        <DatabaseIcon size={18} /> Daten
      </div>
      <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <span className="muted small">
          Ein Backup ist eine gepackte Kopie der Datenbank (Sessions, Piloten, Einstellungen, API-Schlüssel, Hooks; ohne die Pegel-Dateien) im Ordner{' '}
          <code>{info?.dir ?? 'backups'}</code>. Sessions einzeln oder mehrere exportieren: auf der Seite Sessions.
        </span>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <button className="btn btn-secondary" disabled={busy} onClick={backup}>
            <ArchiveIcon size={16} /> Backup erstellen
          </button>
          <button className="btn btn-danger" disabled={busy || running} onClick={() => setConfirmReset(true)} title={running ? 'Erst die Session stoppen' : ''}>
            <TrashIcon size={16} /> Alle Sessions löschen
          </button>
        </div>
        {msg && <span className="small">{msg}</span>}
        {error && <p className="error">{error}</p>}
      </div>
      {info && !info.backups.length && <div className="empty">Noch keine Backups.</div>}
      <div style={{ maxHeight: 300, overflowY: 'auto' }}>
        {info?.backups.map((b) => (
          <div key={b.name} className="list-row">
            <span style={{ display: 'flex', flexDirection: 'column', minWidth: 0, flex: 1, padding: '6px 0' }}>
              <span className="small" style={{ overflowWrap: 'anywhere' }}>
                {b.name}
              </span>
              <span className="muted small">
                {dateTime(b.created_us)} · {size(b.size)}
              </span>
            </span>
            {deleting === b.name ? (
              <button className="btn btn-danger" disabled={busy} onClick={() => remove(b.name)}>
                Wirklich löschen
              </button>
            ) : (
              <>
                <a className="btn btn-ghost btn-icon" href={`/api/backups/${encodeURIComponent(b.name)}`} download title="Herunterladen">
                  <DownloadSimpleIcon size={18} />
                </a>
                <button className="btn btn-ghost btn-icon" title="Backup löschen" onClick={() => setDeleting(b.name)}>
                  <TrashIcon size={18} />
                </button>
              </>
            )}
          </div>
        ))}
      </div>
      {confirmReset && (
        <Dialog title="Alle Sessions löschen?" onClose={() => setConfirmReset(false)}>
          <p className="muted" style={{ margin: 0 }}>
            Alle Trainings und Rennen werden mit ihren Runden und Pegel-Dateien gelöscht, damit auch Bestzeiten und Siege. Piloten, Einstellungen, API-Schlüssel
            und Hooks bleiben. Vorher wird automatisch ein Backup erstellt.
          </p>
          {error && <p className="error">{error}</p>}
          <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
            <button className="btn btn-secondary" onClick={() => setConfirmReset(false)}>
              Behalten
            </button>
            <button className="btn btn-danger" disabled={busy} onClick={reset}>
              Alle löschen
            </button>
          </div>
        </Dialog>
      )}
    </div>
  )
}
