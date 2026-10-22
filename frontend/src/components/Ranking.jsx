// @ts-check
// A session's result table: place, pilot, laps, total time, best lap, and
// every lap time (the best one marked).

import { secs, slotColor } from '../format.js'
import { standings } from '../model.js'

/** @param {{detail: import('../types').SessionDetail}} props */
export default function Ranking({ detail }) {
  const { session: s, results } = detail
  const st = standings(s, results)
  const rows = s.seats.map((seat, i) => ({ seat, r: results[i], st: st[i] }))
  rows.sort((a, b) => (a.st.pos ?? 99) - (b.st.pos ?? 99))
  const overallBest = Math.min(...results.map((r) => r.best_lap_us ?? Infinity))
  return (
    <>
      <div style={{ overflowX: 'auto' }}>
        <table className="table" style={{ minWidth: 380 }}>
          <thead>
            <tr>
              <th>#</th>
              <th>Pilot</th>
              <th>Runden</th>
              {s.config.race && <th>Gesamt</th>}
              <th>Bestzeit</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ seat, r, st }) => (
              <tr key={seat.channel}>
                <td style={{ color: st.pos === 1 ? 'var(--accent)' : 'var(--n500)' }}>{st.pos ?? '–'}</td>
                <td>
                  <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8, fontWeight: 500 }}>
                    <span className="bar" style={{ background: slotColor(seat.channel) }} />
                    {seat.label}
                  </span>
                </td>
                <td>{r.laps.length}</td>
                {s.config.race && <td>{secs(r.total_us)}</td>}
                <td style={{ color: r.best_lap_us === overallBest ? 'var(--accent)' : undefined }}>{secs(r.best_lap_us)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        <span className="muted small">Rundenzeiten</span>
        {rows
          .filter(({ r }) => r.laps.length)
          .map(({ seat, r }) => (
            <div key={seat.channel} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span className="muted small" style={{ width: 80, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                {seat.label}
              </span>
              <div className="lap-chips">
                {r.laps.map((l) => (
                  <span key={l.pass_id} className={l.duration_us === r.best_lap_us ? 'best' : ''}>
                    {secs(l.duration_us)}
                  </span>
                ))}
              </div>
            </div>
          ))}
      </div>
    </>
  )
}
