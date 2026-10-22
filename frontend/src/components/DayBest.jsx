// @ts-check
// Today's best laps, for the training on the side.

import { ArrowRight, Trophy } from '@phosphor-icons/react'
import { secs } from '../format.js'

/** @param {{stats: import('../types').Stats | null, onAll: () => void}} props */
export default function DayBest({ stats, onAll }) {
  const rows = (stats?.today ?? []).slice(0, 8)
  return (
    <div className="panel">
      <div className="panel-head">
        <Trophy size={18} />
        Tagesbestzeiten
        <button className="btn btn-ghost end" style={{ minHeight: 36, fontSize: 13 }} onClick={onAll}>
          Alle Sessions <ArrowRight />
        </button>
      </div>
      {!rows.length && <div className="empty">Heute noch keine Runden.</div>}
      {rows.map((d, k) => (
        <div className="list-row" key={(d.pilot_id ?? '') + d.label}>
          <span className="small" style={{ width: 18, color: k === 0 ? 'var(--accent)' : 'var(--n500)' }}>
            {k + 1}
          </span>
          <span style={{ flex: 1, fontWeight: 500 }}>{d.label}</span>
          {d.races > 0 && (
            <span className="muted small">
              {d.wins} {d.wins === 1 ? 'Sieg' : 'Siege'}
            </span>
          )}
          <span style={{ width: 60, textAlign: 'right', color: k === 0 ? 'var(--accent)' : 'var(--n300)' }}>{secs(d.best_lap_us)}</span>
        </div>
      ))}
    </div>
  )
}
