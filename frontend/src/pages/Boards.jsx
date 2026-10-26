// @ts-check
// The boards and the gates: which gate each board watches (finish, arming,
// off), and per gate how passes are detected (automatic or manual
// thresholds). The manual thresholds themselves are set on race control,
// slot by slot, as on RotorHazard.

import { ArrowsClockwiseIcon, CpuIcon, FlagCheckeredIcon, PencilSimpleIcon, ShieldCheckIcon } from '@phosphor-icons/react'
import { useState } from 'react'
import { api, saveSettings } from '../api.js'
import FirmwarePanel from '../components/FirmwarePanel.jsx'
import { channelMode } from '../model.js'

/** The bar spans this range, dBFS. */
const BAR_MIN = -64
const BAR_MAX = -5
/** @param {number | null | undefined} db */
const pct = (db) => (db == null ? 0 : Math.max(0, Math.min(100, ((db - BAR_MIN) / (BAR_MAX - BAR_MIN)) * 100)))
/** The end of the id (the MAC's last three bytes). @param {string} id */
const shortId = (id) => (id.length > 8 ? '…' + id.slice(-8) : id)

/** @param {{state: import('../types').State, live: import('../types').Live | null, flash: import('../types').FlashStatus | null}} props */
export default function Boards({ state, live, flash }) {
  const s = state.settings
  const boards = live?.boards ?? state.boards
  /** @param {Partial<import('../types').Settings>} patch */
  const save = (patch) => saveSettings(s, patch)
  /** @param {string} id @param {import('../types').BoardRole} role */
  const setRole = (id, role) => save({ board_roles: { ...s.board_roles, [id]: role } })
  const count = (/** @type {import('../types').BoardRole} */ role) => boards.filter((b) => b.role === role && !b.failed).length
  const [scanning, setScanning] = useState(false)
  const [scanError, setScanError] = useState('')

  /** Looks for boards plugged in since the server started (restarts all boards). */
  async function rescan() {
    setScanning(true)
    setScanError('')
    try {
      await api('/boards/rescan', { body: {} })
    } catch (e) {
      setScanError(/** @type {Error} */ (e).message)
    } finally {
      setScanning(false)
    }
  }

  return (
    <>
      <div className="panel">
        <div className="panel-head">
          <CpuIcon size={18} /> Boards
          <span className="end muted small">Mit dem Quad (Sender an) davor wedeln: der Balken zeigt, welches Board welches ist.</span>
          <button className="btn btn-secondary" disabled={scanning || state.active != null} onClick={rescan}>
            <ArrowsClockwiseIcon size={16} /> {scanning ? 'Suche …' : 'Boards neu suchen'}
          </button>
        </div>
        {scanError && <div className="error small" style={{ padding: '0 16px 12px' }}>{scanError}</div>}
        {!boards.length && <div className="empty">Keine Boards gefunden – ESPs anstecken und „Boards neu suchen“ tippen.</div>}
        {boards.map((b) => (
          <div key={b.id} className="thresh-row" style={{ padding: '12px 16px' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
              <span className={b.failed ? 'status-dot bad' : 'status-dot'} />
              <BoardName board={b} disabled={state.active != null} />
              <span className="muted small" style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {b.name ? `${shortId(b.id)} · ` : ''}
                {b.port}
              </span>
              <div className="seg" style={{ flex: 'none' }}>
                {/** @type {const} */ ([
                  ['finish', 'Ziel'],
                  ['arming', 'Freigabe'],
                  ['off', 'Aus'],
                ]).map(([r, label]) => (
                  <button key={r} className={b.role === r ? 'on' : ''} onClick={() => setRole(b.id, r)}>
                    {label}
                  </button>
                ))}
              </div>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <div className="level-bar">
                <div className="fill" style={{ width: pct(b.level_db) + '%' }} />
              </div>
              <span className="muted small tnum" style={{ width: 76, textAlign: 'right' }}>
                {b.level_db != null ? `${b.level_db.toFixed(1)} dBFS` : b.role === 'off' ? 'aus' : '–'}
              </span>
            </div>
            <span className="muted small">
              {b.levels_per_s.toFixed(0)} Messungen/s · {b.stats?.lost ?? 0} verloren
              {b.stats ? ` · Uhr ±${(b.stats.clock_uncertainty_us / 1000).toFixed(1)} ms` : ''}
            </span>
            {b.failed && <span className="error small">{b.failed}</span>}
          </div>
        ))}
      </div>

      <div className="two-col">
        <Gate
          icon={<FlagCheckeredIcon size={18} />}
          title="Zielgate"
          gate="finish"
          state={state}
          boards={count('finish')}
          note={
            count('finish') === 0
              ? { error: true, text: 'Kein Board am Zielgate – es werden keine Runden gezählt.' }
              : count('finish') === 1
                ? { error: false, text: 'Nur ein Board: Automatisch funktioniert, zwei Boards sind aber deutlich zuverlässiger.' }
                : null
          }
        />
        <Gate
          icon={<ShieldCheckIcon size={18} />}
          title="Freigabe-Gate"
          gate="arming"
          state={state}
          boards={count('arming')}
          note={
            count('arming') === 0
              ? { error: false, text: 'Kein Board als Freigabe eingestellt: jeder Durchflug am Ziel zählt.' }
              : {
                  error: false,
                  text: 'Ein Durchflug am Ziel zählt erst, wenn der Pilot seit dem letzten gezählten durchs Freigabe-Gate geflogen ist (der erste nach dem Start zählt immer). Abgelehnte lassen sich in der Nachbearbeitung wiederherstellen.',
                }
          }
        />
      </div>

      <FirmwarePanel state={state} flash={flash} />
    </>
  )
}

/**
 * The board's name (saved on the board itself, so it stays with the board
 * across Pis and USB ports), or the end of its id without one. The pencil
 * edits it; saving restarts all boards for a moment.
 * @param {{board: import('../types').Board, disabled: boolean}} props
 */
function BoardName({ board, disabled }) {
  const [draft, setDraft] = useState(/** @type {string | null} */ (null))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  async function save() {
    if (draft == null) return
    setBusy(true)
    setError('')
    try {
      await api('/boards/name', { body: { id: board.id, name: draft.trim() } })
      setDraft(null)
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    } finally {
      setBusy(false)
    }
  }

  if (draft == null) {
    return (
      <>
        <span style={{ fontWeight: 600 }} className={board.name ? '' : 'tnum'} title={board.id}>
          {board.name ?? shortId(board.id)}
        </span>
        <button
          className="btn btn-ghost btn-icon"
          disabled={disabled}
          title={disabled ? 'Während einer Session nicht möglich' : 'Namen ändern'}
          aria-label="Namen ändern"
          onClick={() => setDraft(board.name ?? '')}
        >
          <PencilSimpleIcon size={16} />
        </button>
      </>
    )
  }
  return (
    <form
      style={{ display: 'flex', gap: 8, flexWrap: 'wrap', flexBasis: '100%', alignItems: 'center' }}
      onSubmit={(e) => {
        e.preventDefault()
        save()
      }}
    >
      <input
        className="input"
        style={{ flex: 1, minWidth: 160 }}
        autoFocus
        maxLength={32}
        value={draft}
        placeholder="z. B. Gate links"
        onChange={(e) => setDraft(e.target.value)}
      />
      <button type="submit" className="btn btn-primary" disabled={busy}>
        {busy ? 'Speichern …' : 'Speichern'}
      </button>
      <button type="button" className="btn btn-ghost" disabled={busy} onClick={() => setDraft(null)}>
        Abbrechen
      </button>
      <span className="muted small" style={{ flexBasis: '100%' }}>
        Wird auf dem Board gespeichert (bleibt auch nach dem Flashen). Leer = kein Name. Die Boards starten dafür kurz neu.
      </span>
      {error && <span className="error small" style={{ flexBasis: '100%' }}>{error}</span>}
    </form>
  )
}

/**
 * One gate: its boards and how its channels are detected. Each channel's
 * mode is set on race control (tap its slot); here all at once.
 * @param {{icon: React.ReactNode, title: string, gate: 'finish' | 'arming', state: import('../types').State, boards: number, note: {error: boolean, text: string} | null}} props
 */
function Gate({ icon, title, gate, state, boards, note }) {
  const s = state.settings
  const d = s.detection
  /** All channels to one mode: the gate's mode, the channels' own modes cleared.
   * @param {import('../types').DetectMode} mode */
  function all(mode) {
    const list = s.channels.map((_, j) => ({ ...(d[gate][j] ?? { enter_db: -25, exit_db: -30 }), mode: null }))
    save({ detection: { ...d, [gate === 'finish' ? 'finish_mode' : 'arming_mode']: mode, [gate]: list } })
  }
  /** @param {Partial<import('../types').Settings>} patch */
  const save = (patch) => saveSettings(s, patch)
  return (
    <div className="panel">
      <div className="panel-head">
        {icon} {title}
        <span className="end muted small">
          {boards} Board{boards === 1 ? '' : 's'}
        </span>
      </div>
      <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          {s.channels.map((c, i) => (
            <span key={i} className="tag" style={channelMode(d, gate, i) === 'classic' ? { background: 'var(--accent-800)' } : undefined}>
              {c.name} · {channelMode(d, gate, i) === 'classic' ? 'manuell' : 'auto'}
            </span>
          ))}
        </div>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <button className="btn btn-secondary" onClick={() => all('auto')}>
            Alle automatisch
          </button>
          <button className="btn btn-secondary" onClick={() => all('classic')}>
            Alle manuell
          </button>
        </div>
        <span className="muted small">Einzelne Kanäle und ihre Enter/Exit-Schwellen: in der Rennleitung den Slot antippen.</span>
        {note && (
          <span className={note.error ? 'error small' : 'small'} style={note.error ? undefined : { color: 'var(--accent-300)' }}>
            {note.text}
          </span>
        )}
      </div>
    </div>
  )
}
