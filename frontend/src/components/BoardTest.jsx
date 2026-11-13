// @ts-check
// Boards → Sensortest: a few seconds of every board's levels, the boards side
// by side per channel (server `boardtest.rs`, judging in `boardTest.js`).
// Without transmitter: the noise floor (a damaged input or a spur). With one
// reference transmitter at a fixed spot: whether a board receives weaker.

import { useEffect, useState } from 'react'
import { CheckCircleIcon, PlayIcon, StethoscopeIcon, WarningIcon, XCircleIcon } from '@phosphor-icons/react'
import { api } from '../api.js'
import { judge, testBoardLabel } from '../boardTest.js'

const KINDS = /** @type {const} */ ([
  ['noise', 'Ohne Sender'],
  ['reference', 'Mit Referenz-Sender'],
])
const DURATIONS = [10, 30]

/** @param {{state: import('../types').State}} props */
export default function BoardTest({ state }) {
  const [kind, setKind] = useState(/** @type {'noise' | 'reference'} */ ('noise'))
  const [seconds, setSeconds] = useState(10)
  const [result, setResult] = useState(/** @type {import('../boardTest.js').BoardTestResult | null} */ (null))
  const [error, setError] = useState('')
  const running = result?.running ?? false

  // The last result (also one started elsewhere); while running, every 0.5 s.
  useEffect(() => {
    let stop = false
    /** @type {ReturnType<typeof setTimeout> | undefined} */
    let timer
    const load = () =>
      api('/boards/test')
        .then((/** @type {import('../boardTest.js').BoardTestResult | null} */ r) => {
          if (stop) return
          setResult(r)
          if (r?.running) timer = setTimeout(load, 500)
        })
        .catch((e) => !stop && setError(e.message))
    load()
    return () => {
      stop = true
      clearTimeout(timer)
    }
  }, [running])

  async function start() {
    setError('')
    try {
      setResult(await api('/boards/test', { body: { kind, seconds } }))
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    }
  }

  const blocked = state.active != null ? 'Während einer Session nicht.' : !state.boards.length ? 'Keine Boards angesteckt.' : ''
  const v = result && !result.running ? judge(result) : null

  return (
    <>
      <div className="panel">
        <div className="panel-head">
          <StethoscopeIcon size={18} /> Sensortest
          <span className="end muted small">Liest nur mit, was die Boards ohnehin senden; Einstellungen bleiben.</span>
        </div>
        <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div className="seg">
            {KINDS.map(([k, label]) => (
              <button key={k} className={kind === k ? 'on' : ''} disabled={running} onClick={() => setKind(k)}>
                {label}
              </button>
            ))}
          </div>
          {kind === 'noise' ? (
            <p className="muted small" style={{ margin: 0 }}>
              <b>Alle Sender aus</b> (Quads, auch DJI-Brillen; möglichst kein 5,8-GHz-WLAN in der Nähe). Gemessen wird der Rauschboden jedes Boards auf
              jedem Kanal. Ein beschädigter Eingang zeigt sich als höherer oder unruhiger Boden als bei den anderen Boards, eine Störlinie als einzelner
              lauter Kanal.
            </p>
          ) : (
            <p className="muted small" style={{ margin: 0 }}>
              <b>Ein Sender an</b>, auf einem eingestellten Kanal, an einer festen Stelle 2–5 m von den Boards entfernt (am besten ein analoger VTX mit fester
              Leistung, kein DJI). Alle Boards gleich ausgerichtet, gleiche Antennen. Für einen sicheren Vergleich: danach die Boards die Plätze tauschen
              lassen und noch einmal testen. Was am Board liegt, wandert mit; was am Platz liegt, nicht.
            </p>
          )}
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
            <div className="seg" style={{ flex: 'none' }}>
              {DURATIONS.map((s) => (
                <button key={s} style={{ padding: '0 12px', whiteSpace: 'nowrap' }} className={seconds === s ? 'on' : ''} disabled={running} onClick={() => setSeconds(s)}>
                  {s} s
                </button>
              ))}
            </div>
            <button className="btn btn-primary" disabled={running || !!blocked} onClick={start}>
              <PlayIcon size={16} /> {running ? 'Test läuft …' : 'Test starten'}
            </button>
            {blocked && <span className="muted small">{blocked}</span>}
            {!blocked && state.replay && <span className="muted small">Wiedergabe: getestet wird die Aufnahme, nicht die angesteckten Boards.</span>}
          </div>
          {running && result && (
            <div className="level-bar" title="Fortschritt">
              <div className="fill" style={{ width: `${Math.min(100, (result.elapsed_s / result.seconds) * 100)}%` }} />
            </div>
          )}
          {error && <p className="error small" style={{ margin: 0 }}>{error}</p>}
        </div>
      </div>

      {v && result && <Result result={result} verdict={v} />}
    </>
  )
}

/** @param {{result: import('../boardTest.js').BoardTestResult, verdict: import('../boardTest.js').Verdict}} props */
function Result({ result, verdict }) {
  const nCh = Math.max(0, ...result.boards.map((b) => b.channels.length))
  const names = result.boards[0]?.channels.map((c) => c.name) ?? []
  /** Cells with a finding: "board:channel". */
  const marked = new Set(verdict.boards.flatMap((v) => v.findings.filter((f) => f.channel != null).map((f) => `${v.board.index}:${f.channel}`)))
  const best = verdict.sender == null ? null : Math.max(...result.boards.map((b) => b.channels[/** @type {number} */ (verdict.sender)]?.median_db ?? -Infinity))

  return (
    <div className="panel">
      <div className="panel-head">
        Ergebnis · {result.kind === 'noise' ? 'ohne Sender' : 'mit Referenz-Sender'} · {result.seconds.toFixed(0)} s
      </div>
      <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {verdict.notes.map((n) => (
          <p key={n} className="small" style={{ margin: 0, color: 'var(--warn)' }}>
            {n}
          </p>
        ))}
        {verdict.boards.map((v) => {
          const bad = v.findings.some((f) => f.level === 'bad')
          const Icon = bad ? XCircleIcon : v.findings.length ? WarningIcon : CheckCircleIcon
          const color = bad ? 'var(--bad)' : v.findings.length ? 'var(--warn)' : 'var(--ok)'
          return (
            <div key={v.board.id} style={{ display: 'flex', gap: 10, alignItems: 'flex-start' }}>
              <Icon size={20} style={{ color, flex: 'none', marginTop: 1 }} weight="fill" />
              <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
                <span>
                  <b>{v.label}</b>{' '}
                  <span className="muted small">
                    {v.board.role === 'finish' ? 'Ziel' : v.board.role === 'arming' ? 'Freigabe' : v.board.role === 'off' ? 'aus' : ''}
                    {v.board.clock_uncertainty_us != null ? ` · Uhr ±${(v.board.clock_uncertainty_us / 1000).toFixed(1)} ms` : ''}
                  </span>
                </span>
                {v.findings.length ? (
                  v.findings.map((f, i) => (
                    <span key={i} className="small" style={{ color: f.level === 'bad' ? 'var(--bad)' : undefined }}>
                      {f.text}
                    </span>
                  ))
                ) : (
                  <span className="muted small">Unauffällig.</span>
                )}
              </div>
            </div>
          )
        })}

        <div style={{ overflowX: 'auto' }}>
          <table className="table">
            <thead>
              <tr>
                <th>Kanal</th>
                {result.boards.map((b) => (
                  <th key={b.id} style={{ textAlign: 'right' }}>
                    {testBoardLabel(b)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {Array.from({ length: nCh }, (_, c) => (
                <tr key={c} style={c === verdict.sender ? { background: 'var(--n900)' } : undefined}>
                  <td>
                    {names[c]} <span className="muted small">{result.boards[0]?.channels[c]?.mhz}</span>
                    {c === verdict.sender && <span className="muted small"> · Sender</span>}
                  </td>
                  {result.boards.map((b) => {
                    const ch = b.channels[c]
                    const odd = marked.has(`${b.index}:${c}`)
                    return (
                      <td key={b.id} className="tnum" style={{ textAlign: 'right', color: odd ? 'var(--warn)' : undefined, whiteSpace: 'nowrap' }}>
                        {ch?.median_db != null ? ch.median_db.toFixed(1) : '–'}
                        {c === verdict.sender && best != null && ch?.median_db != null && ch.median_db < best && (
                          <span className="small"> ({(ch.median_db - best).toFixed(1)})</span>
                        )}
                        <br />
                        <span className="muted small">
                          {ch?.p10_db != null && ch.p90_db != null ? `${ch.p10_db.toFixed(0)}…${ch.p90_db.toFixed(0)}` : ''} · {ch ? ch.per_s.toFixed(0) : 0}/s
                        </span>
                      </td>
                    )
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <p className="muted small" style={{ margin: 0 }}>
          Je Zelle: Median in dBFS, darunter der Bereich von leise (10 %) bis laut (90 %) und die Messwerte pro Sekunde. Gelb: auffällig.
          {result.kind === 'reference' ? ' In der Sender-Zeile in Klammern: Abstand zum besten Board.' : ''}
        </p>
      </div>
    </div>
  )
}
