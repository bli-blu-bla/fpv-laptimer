// @ts-check
// Where the levels come from (Boards → Einrichtung): the boards, or for
// testing a recording or a session's levels played in a loop as if live
// (`PUT /api/source`). Either or: while a recording plays, the boards are
// stopped. A restart of the laptimer goes back to the boards.

import { FilmStripIcon, StopIcon } from '@phosphor-icons/react'
import { useState } from 'react'
import { api } from '../api.js'
import { recLabel } from '../format.js'
import LevelFileList from './LevelFileList.jsx'

/** @param {{state: import('../types').State}} props */
export default function SourcePanel({ state }) {
  const replay = state.replay
  const [picking, setPicking] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const locked = state.active != null

  /** @param {string | null} source  "rec:<name>", "session:<id>"; null: the boards */
  async function play(source) {
    setBusy(true)
    setError('')
    try {
      await api('/source', { method: 'PUT', body: { replay: source } })
      setPicking(false)
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    } finally {
      setBusy(false)
    }
  }

  // The recording's channels replace the set ones by position.
  const set = state.settings.channels.map((c) => `${c.name}@${c.mhz}`)
  const differ = replay != null && replay.channels.length > 0 && replay.channels.join(' ') !== set.join(' ')
  const shown = (/** @type {string} */ c) => c.split('@')[0]

  return (
    <div className="panel">
      <div className="panel-head">
        <FilmStripIcon size={18} /> Quelle
        <span className="end muted small">Zum Testen ohne Fliegen: eine Aufnahme läuft in Schleife wie live, statt der Boards.</span>
        <div className="seg" style={{ flex: 'none' }}>
          <button className={replay == null && !picking ? 'on' : ''} disabled={busy || locked} onClick={() => (replay ? play(null) : setPicking(false))}>
            Boards
          </button>
          <button className={replay != null || picking ? 'on' : ''} disabled={busy || locked} onClick={() => setPicking(true)}>
            Aufnahme
          </button>
        </div>
      </div>
      {(replay || picking || error) && (
        <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          {error && <div className="error small">{error}</div>}
          {locked && <div className="muted small">Während einer Session lässt sich die Quelle nicht wechseln.</div>}
          {replay && !picking && (
            <>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                <span className="status-dot warn" />
                <span style={{ fontWeight: 600 }}>{replay.source.startsWith('rec:') ? `Aufnahme ${recLabel(replay.label)}` : replay.label}</span>
                <span className="muted small" style={{ flex: 1 }}>
                  läuft in Schleife{replay.channels.length ? ` · Kanäle ${replay.channels.map(shown).join(', ')}` : ''}
                </span>
                {replay.source !== 'file' && (
                  <button className="btn btn-ghost" disabled={busy || locked} onClick={() => setPicking(true)}>
                    Andere wählen
                  </button>
                )}
                <button className="btn btn-secondary" disabled={busy || locked} onClick={() => play(null)}>
                  <StopIcon size={16} /> Zurück zu den Boards
                </button>
              </div>
              {differ && (
                <div className="small" style={{ color: 'var(--warn)' }}>
                  Eingestellt sind {set.map(shown).join(', ')}: die Pegel kommen nach Position, Slot 1 bekommt den ersten Kanal der Aufnahme usw.
                </div>
              )}
            </>
          )}
          {picking && <LevelFileList action="Abspielen" disabled={busy || locked} current={replay?.source ?? null} onPick={(source) => play(source)} />}
        </div>
      )}
    </div>
  )
}
