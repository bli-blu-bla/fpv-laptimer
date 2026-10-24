// @ts-check
// Settings that rarely change, in three tabs (after the "Einstellungen"
// design): measuring (which channels are scanned, chosen in ChannelPicker
// and rated for intermodulation; minimum lap, swing, gain), the
// announcements, and the system (version, updates, restart: SystemPanel;
// login; backups; deleting all sessions). (Boards and gates: their own page;
// slots, race format and manual thresholds: race control; exporting
// sessions: the sessions page.)

import { useEffect, useState } from 'react'
import {
  ArchiveIcon,
  BroadcastIcon,
  CheckCircleIcon,
  CircleNotchIcon,
  CodeIcon,
  CpuIcon,
  DatabaseIcon,
  DownloadSimpleIcon,
  FadersIcon,
  FileZipIcon,
  FlagCheckeredIcon,
  HourglassMediumIcon,
  LockIcon,
  MicrophoneIcon,
  MinusCircleIcon,
  PencilSimpleIcon,
  PlayIcon,
  SignOutIcon,
  SlidersIcon,
  SpeakerHighIcon,
  StarIcon,
  TimerIcon,
  TrashIcon,
  TrophyIcon,
  WarningIcon,
  WaveSineIcon,
} from '@phosphor-icons/react'
import { api, saveSettings } from '../api.js'
import { isSpeaker, setSpeaker } from '../announcer.js'
import { logout, useAuth } from '../auth.js'
import { dateTime, slotColor } from '../format.js'
import { assignSlots, channelName, imdRating, minSpacing } from '../imd.js'
import ChannelPicker, { imdText } from '../components/ChannelPicker.jsx'
import DetectPreview from '../components/DetectPreview.jsx'
import ImdBadge from '../components/ImdBadge.jsx'
import Stepper from '../components/Stepper.jsx'
import SystemPanel from '../components/SystemPanel.jsx'
import Tabs from '../components/Tabs.jsx'

/** Closer channels get a warning on their slot (and on the tab). */
const CLOSE_MHZ = 30

/** @param {{state: import('../types').State, onOpenApi: () => void}} props */
export default function SettingsPage({ state, onOpenApi }) {
  const s = state.settings
  const running = state.active != null
  const [error, setError] = useState('')
  const [pending, setPending] = useState(0)
  const [tab, setTab] = useState(/** @type {'measure' | 'announce' | 'system'} */ ('measure'))
  /** @param {Partial<import('../types').Settings>} patch */
  const save = (patch) => {
    setError('')
    setPending((n) => n + 1)
    return saveSettings(s, patch)
      .catch((e) => setError(e.message))
      .finally(() => setPending((n) => n - 1))
  }

  const mhz = s.channels.map((c) => c.mhz)
  const imd = imdRating(mhz)
  const channelWarn = (imd.rating != null && imd.rating < 50) || minSpacing(mhz) < CLOSE_MHZ

  const tabs = /** @type {const} */ ([
    ['measure', 'Messung', SlidersIcon, channelWarn],
    ['announce', 'Ansagen', SpeakerHighIcon, false],
    ['system', 'System', CpuIcon, false],
  ])
  const saved = pending ? (
    <span className="muted">
      <CircleNotchIcon size={15} className="spin" /> <span className="wide-only">Speichert …</span>
    </span>
  ) : (
    <span className="saved">
      <CheckCircleIcon size={15} /> <span className="wide-only">Gespeichert</span>
    </span>
  )

  return (
    <>
      <Tabs tabs={tabs} value={tab} onChange={setTab} end={saved} />
      {error && <p className="error">{error}</p>}

      {tab === 'measure' && (
        <div className="settings-grid">
          <ChannelsPanel settings={s} running={running} save={save} />
          <div className="settings-col">
            <div className="panel">
              <div className="panel-head">
                <FadersIcon size={18} /> Auswertung
              </div>
              <div className="panel-body param-grid">
                <Stepper
                  box
                  label="Mindest-Rundenzeit"
                  value={s.min_lap_s}
                  unit="s"
                  step={0.5}
                  min={1}
                  max={60}
                  hint="Schnellere Durchflüge werden ignoriert."
                  onSet={(v) => save({ min_lap_s: v })}
                />
                <Stepper
                  box
                  label="Mindestausschlag"
                  value={s.detection.min_rise_db}
                  unit="dB"
                  step={1}
                  min={3}
                  max={30}
                  hint="Höher, wenn Überflüge zählen; niedriger, wenn echte fehlen."
                  onSet={(v) => save({ detection: { ...s.detection, min_rise_db: v } })}
                />
                <Stepper
                  box
                  label="Verstärkung (Gain)"
                  value={s.gain}
                  step={1}
                  min={0}
                  max={60}
                  hint="30 bei 25 mW. Kleiner, wenn das Signal am Gate übersteuert."
                  delayed
                  onSet={(v) => save({ gain: v })}
                />
                <Stepper
                  box
                  label="Beste Runden in Folge"
                  value={s.consecutive}
                  step={1}
                  min={2}
                  max={20}
                  hint={`Für die Wertung „Beste ${s.consecutive}“ in Bestzeiten.`}
                  onSet={(v) => save({ consecutive: v })}
                />
              </div>
            </div>
            <div className="panel">
              <div className="panel-head">
                <WaveSineIcon size={18} /> Vorschau
                <span className="end muted small">So wirkt der Mindestausschlag</span>
              </div>
              <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                <DetectPreview riseDb={s.detection.min_rise_db} />
                <div className="legend">
                  <span>
                    <span className="legend-line" style={{ borderColor: 'var(--n500)' }} /> Normalpegel
                  </span>
                  <span>
                    <span className="legend-line" style={{ borderColor: 'var(--accent)' }} /> Auslöseschwelle (+{s.detection.min_rise_db} dB)
                  </span>
                  <span>
                    <span className="legend-box" /> Gezählt
                  </span>
                </div>
                <span className="muted small">
                  Im automatischen Modus lernt die Erkennung Normalpegel und Gipfel aus dem Flug; der Mindestausschlag ist die Untergrenze. Kanäle mit eigenen
                  Schwellen stellst du in der Rennleitung ein.
                </span>
              </div>
            </div>
          </div>
        </div>
      )}

      {tab === 'announce' && <AnnouncePanel settings={s} save={save} />}

      {tab === 'system' && (
        <div className="settings-grid">
          <div className="settings-col">
            <SystemPanel version={state.version} running={running} />
            <AccessPanel />
          </div>
          <div className="settings-col">
            <DataPanel running={running} />
            {/* On phones the API page has no tab of its own. */}
            <button className="btn btn-secondary narrow-only" onClick={onOpenApi}>
              <CodeIcon size={16} /> API & Hooks
            </button>
          </div>
        </div>
      )}
    </>
  )
}

/** The scanned channels: a strip with their positions, the IMD rating, one
 * row per slot; "Kanäle wählen" opens the band chart.
 * @param {{settings: import('../types').Settings, running: boolean, save: (patch: Partial<import('../types').Settings>) => Promise<void>}} props */
function ChannelsPanel({ settings: s, running, save }) {
  const [picking, setPicking] = useState(false)
  const mhz = s.channels.map((c) => c.mhz)
  const imd = imdRating(mhz)
  const spacing = minSpacing(mhz)
  const good = (imd.rating == null || imd.rating >= 50) && spacing >= CLOSE_MHZ

  /** Takes the chosen frequencies; slots keep their pilot and detection
   * settings where their frequency stays (assignSlots). @param {number[]} next */
  function apply(next) {
    const plan = assignSlots(mhz, next)
    /** Moves a per-slot list along; stops at the first slot without an entry
     * (the server fills in defaults). @template V @param {V[]} list */
    const carry = (list) => {
      /** @type {V[]} */
      const out = []
      for (const p of plan) {
        const v = p.from != null ? list[p.from] : undefined
        if (v === undefined) break
        out.push(v)
      }
      return out
    }
    save({
      channels: plan.map((p) => ({ name: channelName(p.mhz) ?? String(p.mhz), mhz: p.mhz })),
      seats: plan.map((p) => (p.from != null ? (s.seats[p.from] ?? defSeat()) : defSeat())),
      detection: { ...s.detection, finish: carry(s.detection.finish), arming: carry(s.detection.arming) },
    }).then(() => setPicking(false))
  }
  /** @param {number} i */
  const remove = (i) => apply(mhz.filter((_, j) => j !== i))

  // The strip covers the 5.8 GHz bands, wider if a channel lies outside.
  const lo = Math.min(5645, ...mhz)
  const hi = Math.max(5945, ...mhz)
  /** @param {number} f */
  const pos = (f) => `${((f - lo) / (hi - lo)) * 100}%`

  return (
    <div className="panel">
      <div className="panel-head" style={{ flexWrap: 'wrap' }}>
        <BroadcastIcon size={18} /> Kanäle
        <span className="tag">{s.channels.length}&nbsp;von 8</span>
        <button className="btn btn-secondary end" disabled={running} onClick={() => setPicking(true)} title={running ? 'Erst die Session stoppen' : ''}>
          <PencilSimpleIcon size={16} /> Kanäle wählen
        </button>
      </div>
      <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        <div className="freq-strip">
          <div className="freq-marks">
            <div className="freq-line" />
            {s.channels.map((c, i) => (
              <div key={i} className="freq-mark" style={{ left: pos(c.mhz), color: slotColor(i) }}>
                <span className="tnum">{c.name}</span>
                <span className="pip" style={{ background: slotColor(i) }} />
              </div>
            ))}
          </div>
          <div className="freq-range tnum">
            <span>{lo} MHz</span>
            <span>{hi} MHz</span>
          </div>
          <div className="freq-status" style={{ color: good ? 'var(--ok)' : 'var(--warn)' }}>
            {good ? <CheckCircleIcon size={16} /> : <WarningIcon size={16} />}
            <span style={{ flex: 1 }}>
              {spacing === 0
                ? 'Zwei Slots auf derselben Frequenz.'
                : spacing < CLOSE_MHZ
                  ? `Zwei Kanäle liegen nur ${spacing} MHz auseinander.`
                  : imdText(imd.rating, mhz.length)}
            </span>
            <ImdBadge rating={imd.rating} />
          </div>
        </div>

        {running && <span className="error small">Während einer Session lassen sich Kanäle nicht ändern.</span>}
        <div>
          {s.channels.map((c, i) => {
            const near = Math.min(...mhz.map((f, j) => (j === i ? Infinity : Math.abs(f - c.mhz))))
            return (
              <div key={i} className="slot-row">
                <span className="dot" style={{ background: slotColor(i) }} />
                <span style={{ fontWeight: 500 }}>Slot {i + 1}</span>
                <span className="muted small tnum">
                  {c.name === String(c.mhz) ? `${c.mhz} MHz` : `${c.name} · ${c.mhz} MHz`}
                </span>
                {near < CLOSE_MHZ && <span className="tag tag-warn">{near === 0 ? 'Gleicher Kanal' : `Nur ${near} MHz Abstand`}</span>}
                <button
                  className="btn btn-ghost btn-icon end"
                  disabled={running || s.channels.length <= 1}
                  onClick={() => remove(i)}
                  title="Slot entfernen"
                >
                  <MinusCircleIcon size={18} />
                </button>
              </div>
            )
          })}
        </div>
        <span className="muted small">
          Alle Boards scannen alle Kanäle: weniger Kanäle messen öfter. Wer auf welchem Kanal fliegt, stellst du in der Rennleitung ein: Namen in der Karte antippen.
        </span>
      </div>
      {picking && <ChannelPicker value={mhz} onSave={apply} onClose={() => setPicking(false)} />}
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

/** What is announced (left) and where it is played (right).
 * @param {{settings: import('../types').Settings, save: (patch: Partial<import('../types').Settings>) => void}} props */
function AnnouncePanel({ settings: s, save }) {
  const a = s.announce
  const [speaker, setSpeakerState] = useState(isSpeaker)
  const [testing, setTesting] = useState(false)
  /** @param {Partial<import('../types').AnnounceSettings>} patch */
  const set = (patch) => save({ announce: { ...a, ...patch } })
  /** @param {boolean} on */
  function toggleSpeaker(on) {
    setSpeaker(on)
    setSpeakerState(on)
  }
  function test() {
    setTesting(true)
    api('/announce/test', { method: 'POST' }).finally(() => setTimeout(() => setTesting(false), 1500))
  }
  /** @param {import('@phosphor-icons/react').Icon} Icon @param {string} label @param {string} hint @param {keyof import('../types').AnnounceSettings} key */
  const toggle = (Icon, label, hint, key) => (
    <button className="row-toggle" disabled={!s.sound} onClick={() => set({ [key]: !a[key] })}>
      <Icon size={20} className="row-icon" />
      <span className="text">
        <span>{label}</span>
        <span className="hint">{hint}</span>
      </span>
      <span className={a[key] ? 'switch on' : 'switch'} />
    </button>
  )

  return (
    <div className="settings-grid">
      <div className="panel">
        <div className="panel-head">
          <SpeakerHighIcon size={18} /> Was wird angesagt
        </div>
        <div className="toggle-list">
          <button className="row-toggle" onClick={() => save({ sound: !s.sound })}>
            <SpeakerHighIcon size={20} className="row-icon" />
            <span className="text">
              <span>Ansagen und Töne</span>
              <span className="hint">Alle an oder aus (auch oben rechts)</span>
            </span>
            <span className={s.sound ? 'switch on' : 'switch'} />
          </button>
          {toggle(HourglassMediumIcon, 'Start', '„Piloten, macht euch bereit“, Countdown, Startton; im Training „Training läuft“', 'start')}
          <div className="row-field" style={{ opacity: s.sound ? 1 : 0.45 }}>
            <TimerIcon size={20} className="row-icon" />
            <span className="text">
              <span>Nach jeder Runde</span>
              <span className="hint">{LAP_HINTS[a.lap]}</span>
            </span>
            <div className="seg" style={{ flex: 'none' }}>
              {LAP_CALLS.map(([k, label]) => (
                <button key={k} disabled={!s.sound} className={a.lap === k ? 'on' : ''} onClick={() => set({ lap: k })} style={{ minWidth: 52 }}>
                  {label}
                </button>
              ))}
            </div>
          </div>
          {toggle(StarIcon, 'Neue Bestzeit', 'Training: „Pilot Nitro hat eine neue Bestzeit erzielt: 13,7“ statt der Runden-Ansage', 'best')}
          {toggle(FlagCheckeredIcon, 'Pilot im Ziel', 'Rennen: „Nitro ist im Ziel“', 'finished')}
          {toggle(TrophyIcon, 'Rennen beendet', 'Rennen: wenn alle im Ziel sind', 'race_over')}
        </div>
      </div>

      <div className="panel">
        <div className="panel-head">
          <MicrophoneIcon size={18} /> Ausgabe
        </div>
        <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
          <div className="field">
            <span>Ausgabe auf</span>
            <div className="seg">
              <button className={a.output === 'server' ? 'on' : ''} onClick={() => set({ output: 'server' })}>
                Lautsprecher am Server
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
          <div className="inset" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            <span className="muted small">Beispiel nach einer Runde</span>
            <span style={{ fontSize: 17, fontWeight: 500 }}>{a.lap === 'off' ? 'Keine Rundenansage' : a.lap === 'beep' ? '(Piep)' : LAP_HINTS[a.lap]}</span>
            <button className="btn btn-primary" style={{ alignSelf: 'flex-start' }} disabled={!s.sound || testing} onClick={test}>
              <PlayIcon size={16} /> {testing ? 'Spielt …' : 'Probe-Ansage'}
            </button>
          </div>
          <span className="muted small">
            Fertige Sounds liegen im Ordner <code>sounds/</code>; Namen und Zeiten spricht die Sprachausgabe (auf dem Pi pico2wave oder espeak-ng).
          </span>
        </div>
      </div>
    </div>
  )
}

const defSeat = () => ({ pilot_id: null, label: '', enabled: true })

/** @typedef {{name: string, size: number, created_us: number}} Backup */

/** @param {number} n */
const size = (n) => (n < 1024 * 1024 ? `${Math.max(1, Math.round(n / 1024))} KB` : `${(n / 1024 / 1024).toFixed(1).replace('.', ',')} MB`)

/** Logging out (this device or all). */
function AccessPanel() {
  const { enabled } = useAuth()
  const [error, setError] = useState('')
  /** @param {boolean} everywhere */
  const out = (everywhere) => logout(everywhere).catch((e) => setError(e.message))
  return (
    <div className="panel">
      <div className="panel-head">
        <LockIcon size={18} /> Zugang
      </div>
      <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {enabled ? (
          <>
            <div className="inset" style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
              <span style={{ flex: 1 }}>Dieses Gerät ist angemeldet</span>
              <button className="btn btn-ghost" onClick={() => out(false)}>
                Abmelden
              </button>
            </div>
            <button className="btn btn-secondary" style={{ alignSelf: 'flex-start' }} onClick={() => out(true)}>
              <SignOutIcon size={16} /> Alle Geräte abmelden
            </button>
            <span className="muted small">
              Das Passwort steht in <code>~/.laptimer/config.toml</code> auf dem Server. Nach einer Änderung dort: „Alle Geräte abmelden“.
            </span>
          </>
        ) : (
          <span className="small">
            Ohne Passwort: jeder im Netz kann alles ändern (<code>[auth] enabled = false</code> in <code>~/.laptimer/config.toml</code>).
          </span>
        )}
        {error && <p className="error">{error}</p>}
      </div>
    </div>
  )
}

/** Backups of the database, and deleting all sessions (server/src/data.rs).
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
    <>
      <div className="panel">
        <div className="panel-head" style={{ flexWrap: 'wrap' }}>
          <DatabaseIcon size={18} /> Backups
          <button className="btn btn-primary end" disabled={busy} onClick={backup}>
            <ArchiveIcon size={16} /> Jetzt sichern
          </button>
        </div>
        <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div className="inset backup-list">
            {info && !info.backups.length && (
              <div className="empty" style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <ArchiveIcon size={20} /> Noch keine Backups.
              </div>
            )}
            {info?.backups.map((b) => (
              <div key={b.name} className="list-row">
                <FileZipIcon size={18} className="row-icon" />
                <span style={{ display: 'flex', flexDirection: 'column', minWidth: 0, flex: 1, padding: '6px 0' }}>
                  <span className="small" style={{ overflowWrap: 'anywhere' }}>
                    {b.name}
                  </span>
                  <span className="muted small">
                    {dateTime(b.created_us)} · {size(b.size)}
                  </span>
                </span>
                {deleting === b.name ? (
                  <>
                    <button className="btn btn-ghost" onClick={() => setDeleting(null)}>
                      Behalten
                    </button>
                    <button className="btn btn-danger" disabled={busy} onClick={() => remove(b.name)}>
                      Löschen
                    </button>
                  </>
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
          {msg && <span className="small">{msg}</span>}
          {error && <p className="error">{error}</p>}
          <span className="muted small">
            Gepackte Kopie der Datenbank: Sessions, Piloten, Einstellungen, API-Schlüssel und Hooks, ohne Pegel-Dateien. Ablage: <code>{info?.dir ?? 'backups'}</code>.
            Sessions exportieren: auf der Seite Sessions.
          </span>
        </div>
      </div>

      <div className="panel danger-zone">
        <div className="panel-head">
          <WarningIcon size={18} /> Gefahrenzone
        </div>
        <div className="panel-body" style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
          <span style={{ flex: 1, minWidth: 200, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
            <span>Alle Sessions löschen</span>
            <span className="muted small">
              {running
                ? 'Erst die Session stoppen.'
                : confirmReset
                  ? 'Alle Trainings und Rennen mit Runden, Pegel-Dateien, Bestzeiten und Siegen. Vorher wird automatisch gesichert.'
                  : 'Piloten, Einstellungen, API-Schlüssel und Hooks bleiben.'}
            </span>
          </span>
          {confirmReset && (
            <button className="btn btn-ghost" onClick={() => setConfirmReset(false)}>
              Abbrechen
            </button>
          )}
          <button
            className={confirmReset ? 'btn btn-danger btn-danger-solid' : 'btn btn-danger'}
            disabled={busy || running}
            onClick={() => (confirmReset ? reset() : setConfirmReset(true))}
          >
            <TrashIcon size={16} /> {confirmReset ? 'Endgültig löschen' : 'Löschen …'}
          </button>
        </div>
      </div>
    </>
  )
}
