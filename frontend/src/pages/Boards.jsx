// @ts-check
// The boards and the gates: which gate each board watches (finish, arming,
// off), and per gate how passes are detected (automatic or manual
// thresholds). The manual thresholds themselves are set on race control,
// slot by slot, as on RotorHazard.

import { Cpu, FlagCheckered, ShieldCheck } from '@phosphor-icons/react'
import { saveSettings } from '../api.js'

/** The bar spans this range, dBFS. */
const BAR_MIN = -64
const BAR_MAX = -5
/** @param {number | null | undefined} db */
const pct = (db) => (db == null ? 0 : Math.max(0, Math.min(100, ((db - BAR_MIN) / (BAR_MAX - BAR_MIN)) * 100)))
/** The end of the id (the MAC's last three bytes). @param {string} id */
const shortId = (id) => (id.length > 8 ? '…' + id.slice(-8) : id)

/** @param {{state: import('../types').State, live: import('../types').Live | null}} props */
export default function Boards({ state, live }) {
  const s = state.settings
  const boards = live?.boards ?? state.boards
  /** @param {Partial<import('../types').Settings>} patch */
  const save = (patch) => saveSettings(s, patch)
  /** @param {string} id @param {import('../types').BoardRole} role */
  const setRole = (id, role) => save({ board_roles: { ...s.board_roles, [id]: role } })
  const count = (/** @type {import('../types').BoardRole} */ role) => boards.filter((b) => b.role === role && !b.failed).length

  return (
    <>
      <div className="panel">
        <div className="panel-head">
          <Cpu size={18} /> Boards
          <span className="end muted small">Mit dem Quad (Sender an) davor wedeln: der Balken zeigt, welches Board welches ist.</span>
        </div>
        {!boards.length && <div className="empty">Keine Boards gefunden – ESPs anstecken und den Server neu starten.</div>}
        {boards.map((b) => (
          <div key={b.id} className="thresh-row" style={{ padding: '12px 16px' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
              <span className={b.failed ? 'status-dot bad' : 'status-dot'} />
              <span style={{ fontWeight: 600 }} className="tnum" title={b.id}>
                {shortId(b.id)}
              </span>
              <span className="muted small" style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
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
          icon={<FlagCheckered size={18} />}
          title="Zielgate"
          boards={count('finish')}
          mode={s.detection.finish_mode}
          onMode={(m) => save({ detection: { ...s.detection, finish_mode: m } })}
          note={
            count('finish') === 0
              ? { error: true, text: 'Kein Board am Zielgate – es werden keine Runden gezählt.' }
              : count('finish') === 1 && s.detection.finish_mode === 'auto'
                ? { error: false, text: 'Nur ein Board: Automatisch funktioniert, zwei Boards sind aber deutlich zuverlässiger.' }
                : null
          }
        />
        <Gate
          icon={<ShieldCheck size={18} />}
          title="Freigabe-Gate"
          boards={count('arming')}
          mode={s.detection.arming_mode}
          onMode={(m) => save({ detection: { ...s.detection, arming_mode: m } })}
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
    </>
  )
}

/**
 * One gate: its boards and its detection mode.
 * @param {{icon: React.ReactNode, title: string, boards: number, mode: 'auto' | 'classic', onMode: (m: 'auto' | 'classic') => void, note: {error: boolean, text: string} | null}} props
 */
function Gate({ icon, title, boards, mode, onMode, note }) {
  return (
    <div className="panel">
      <div className="panel-head">
        {icon} {title}
        <span className="end muted small">
          {boards} Board{boards === 1 ? '' : 's'}
        </span>
      </div>
      <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <div className="seg">
          <button className={mode === 'auto' ? 'on' : ''} onClick={() => onMode('auto')}>
            Automatisch
          </button>
          <button className={mode === 'classic' ? 'on' : ''} onClick={() => onMode('classic')}>
            Manuell (Enter/Exit)
          </button>
        </div>
        <span className="muted small">
          {mode === 'auto'
            ? 'Die Schwellen werden aus dem Flug gelernt, pro Pilot. Nichts einzustellen.'
            : 'Feste Schwellen wie bei RotorHazard. Einstellen in der Rennleitung: Slot antippen, darunter erscheinen Pegelverlauf, Enter und Exit.'}
        </span>
        {note && <span className={note.error ? 'error small' : 'small'} style={note.error ? undefined : { color: 'var(--accent-300)' }}>{note.text}</span>}
      </div>
    </div>
  )
}
