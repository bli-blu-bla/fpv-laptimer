// @ts-check
// A session's result: one row per pilot, in finishing order, with a place
// badge (1–3 highlighted), status (laps, in the finish or not), every lap
// time (the session's fastest marked), total and gap. In a race, pilots
// still out on the course come after a "Nicht im Ziel" line. Design:
// "Rennergebnis Tabelle" on Claude Design. Below 640 px of width each row
// becomes a card (container query in index.css).

import { Fragment } from 'react'
import { secs, slotColor } from '../format.js'
import { standings } from '../model.js'

/** @param {{detail: import('../types').SessionDetail}} props */
export default function Ranking({ detail }) {
  const { session: s, results } = detail
  const race = s.config.race
  const end = s.config.end_rule
  const target = end.kind === 'laps' ? end.value : null
  // Only a race with an end has a finish; an open one runs until stopped.
  const hasFinish = race && end.kind !== 'open'
  const st = standings(s, results)
  const rows = s.seats.map((seat, i) => ({ seat, r: results[i], st: st[i] }))
  rows.sort((a, b) => (a.st.pos ?? 99) - (b.st.pos ?? 99))
  const overallBest = Math.min(...results.map((r) => r.best_lap_us ?? Infinity))
  const fastest = rows.find(({ r }) => r.best_lap_us === overallBest)

  /** @param {import('../types').PilotResult} r */
  const status = (r) => {
    const laps = target != null ? `${r.laps.length}/${target} Runden` : `${r.laps.length} ${r.laps.length === 1 ? 'Runde' : 'Runden'}`
    return hasFinish && r.finished_us != null ? `${laps} · im Ziel` : laps
  }

  return (
    <div className="ranking">
      <div className="ranking-head">
        <span className="small muted">
          {race ? 'Rennen' : 'Training'}
          {target != null && ` · ${target} Runden`}
          {end.kind === 'seconds' && ` · ${end.value} s`}
        </span>
        {fastest && (
          <span className="small muted">
            Schnellste Runde <b className="tnum" style={{ color: 'var(--accent-300)' }}>{secs(overallBest)}</b> · {fastest.seat.label}
          </span>
        )}
      </div>
      <div className="ranking-row ranking-cols" aria-hidden="true">
        <span>Platz</span>
        <span>Pilot</span>
        <span>Rundenzeiten</span>
        <span className="rk-num">{race ? 'Gesamt' : 'Bestzeit'}</span>
        <span className="rk-num">Abstand</span>
      </div>
      {rows.map(({ seat, r, st }, k) => {
        const done = !hasFinish || r.finished_us != null
        const prevDone = k > 0 && (!hasFinish || rows[k - 1].r.finished_us != null)
        const place = st.pos == null ? '' : st.pos <= 3 ? ` rk-p${st.pos}` : ''
        return (
          <Fragment key={seat.channel}>
            {hasFinish && !done && (k === 0 || prevDone) && (
              <div className="ranking-divider">
                <span>Nicht im Ziel</span>
              </div>
            )}
            <div className={`ranking-row${st.pos === 1 ? ' rk-first' : ''}${done ? '' : ' rk-out'}`}>
              <span className={`rk-place${place}`}>{st.pos ?? '–'}</span>
              <span className="rk-pilot">
                <span className="bar" style={{ background: slotColor(seat.channel) }} />
                <span className="rk-who">
                  <span className="rk-name">{seat.label}</span>
                  <span className={`rk-status ${hasFinish ? (done ? 'rk-ok' : 'rk-warn') : ''}`}>{status(r)}</span>
                </span>
              </span>
              <span className="lap-chips">
                {r.laps.map((l) => (
                  <span key={l.pass_id} className={l.duration_us === overallBest ? 'best' : ''}>
                    {secs(l.duration_us)}
                  </span>
                ))}
              </span>
              <span className="rk-num rk-total">{secs(race ? r.total_us : r.best_lap_us)}</span>
              <span className={`rk-num rk-gap${st.pos === 1 ? ' rk-lead' : ''}`}>{st.pos === 1 ? (race && s.ended_us != null ? 'Sieger' : 'Führt') : st.gap}</span>
            </div>
          </Fragment>
        )
      })}
    </div>
  )
}
