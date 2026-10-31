// @ts-check
// Best times: today or all time, per pilot.

import { useHashPart } from '../route.js'
import { secs } from '../format.js'

/** @param {{state: import('../types').State, stats: import('../types').Stats | null}} props */
export default function Best({ state, stats }) {
  const [scope, setScope] = useHashPart(1, /** @type {const} */ (['today', 'all']), 'today')
  const rows = (scope === 'today' ? stats?.today : stats?.all) ?? []
  const n = state.settings.consecutive
  const best = rows[0]?.best_lap_us
  const bestN = Math.min(...rows.map((r) => r.best_consecutive_us ?? Infinity))
  return (
    <>
      <div className="seg" style={{ maxWidth: 320 }}>
        <button className={scope === 'today' ? 'on' : ''} onClick={() => setScope('today')}>
          Heute
        </button>
        <button className={scope === 'all' ? 'on' : ''} onClick={() => setScope('all')}>
          Gesamt
        </button>
      </div>
      <div className="panel" style={{ padding: '4px 8px' }}>
        {!rows.length ? (
          <div className="empty">{scope === 'today' ? 'Heute noch keine Runden.' : 'Noch keine Runden.'}</div>
        ) : (
          <div style={{ overflowX: 'auto' }}>
            <table className="table" style={{ minWidth: 420 }}>
              <thead>
                <tr>
                  <th>#</th>
                  <th>Pilot</th>
                  <th>Bestzeit</th>
                  <th>Beste {n} in Folge</th>
                  <th>Rennen</th>
                  <th>Siege</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r, k) => {
                  const pilot = state.pilots.find((p) => p.id === r.pilot_id)
                  return (
                    <tr key={(r.pilot_id ?? '') + r.label}>
                      <td style={{ color: k === 0 ? 'var(--accent)' : 'var(--n500)' }}>{k + 1}</td>
                      <td>
                        <span style={{ fontWeight: 600 }}>{pilot?.name ?? r.label}</span>
                        {pilot?.full_name && <span className="muted small"> · {pilot.full_name}</span>}
                      </td>
                      <td style={{ color: r.best_lap_us === best ? 'var(--accent)' : undefined, fontSize: 16 }}>{secs(r.best_lap_us)}</td>
                      <td style={{ color: r.best_consecutive_us === bestN ? 'var(--accent)' : undefined }}>{secs(r.best_consecutive_us)}</td>
                      <td>{r.races}</td>
                      <td>{r.wins}</td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </>
  )
}
