// @ts-check
// The frame: navigation (a sidebar on wide screens, a tab bar at the bottom
// on phones), the top bar, and the four pages.

import { useEffect, useState } from 'react'
import { Broadcast, ClockCounterClockwise, SlidersHorizontal, SpeakerHigh, SpeakerSlash, Timer, UsersThree } from '@phosphor-icons/react'
import { api, saveSettings, useServer } from './api.js'
import Live from './pages/Live.jsx'
import Pilots from './pages/Pilots.jsx'
import Sessions from './pages/Sessions.jsx'
import SettingsPage from './pages/Settings.jsx'

const PAGES = /** @type {const} */ ([
  ['live', 'Live', Broadcast],
  ['sessions', 'Sessions', ClockCounterClockwise],
  ['pilots', 'Piloten', UsersThree],
  ['settings', 'Einstellungen', SlidersHorizontal],
])

export default function App() {
  const server = useServer()
  const { state, connected, changed } = server
  const [page, setPage] = useState(/** @type {'live' | 'sessions' | 'pilots' | 'settings'} */ ('live'))
  const stats = useStats(state, changed)

  const boards = state?.boards ?? []
  const failed = boards.filter((b) => b.failed).length
  const lost = boards.reduce((n, b) => n + (b.stats?.lost ?? 0), 0)
  const ok = connected && boards.length > 0 && !failed
  const boardText = !connected ? 'Keine Verbindung' : `${boards.length} Board${boards.length === 1 ? '' : 's'}${failed ? `, ${failed} ausgefallen` : ''}`

  const nav = PAGES.map(([id, label, Icon]) => (
    <button key={id} className={page === id ? 'nav-btn on' : 'nav-btn'} onClick={() => setPage(id)}>
      <Icon size={22} />
      <span>{label}</span>
    </button>
  ))

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-icon">
            <Timer size={18} />
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.15 }}>
            <span style={{ fontWeight: 600 }}>Laptimer</span>
            <span className="muted" style={{ fontSize: 11 }}>
              FPV Zeitmessung
            </span>
          </div>
        </div>
        {nav}
        <div className="board-box">
          <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span className={ok ? 'status-dot' : 'status-dot bad'} />
            {boardText}
          </span>
          <span className="muted">{lost} Messungen verloren</span>
        </div>
      </aside>

      <div className="main">
        <header className="topbar">
          <div style={{ display: 'flex', flexDirection: 'column', minWidth: 0 }}>
            <span className="kicker">
              Heute · {stats?.today_races ?? 0} {stats?.today_races === 1 ? 'Rennen' : 'Rennen'}
            </span>
            <span className="title">{PAGES.find((p) => p[0] === page)?.[1]}</span>
          </div>
          <div className="right">
            <span className="narrow-only" style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <span className={ok ? 'status-dot' : 'status-dot bad'} />
              {boardText}
            </span>
            {state && (
              <button
                className="btn btn-ghost btn-icon"
                title={state.settings.sound ? 'Ansagen aus' : 'Ansagen an'}
                onClick={() => saveSettings(state.settings, { sound: !state.settings.sound })}
              >
                {state.settings.sound ? <SpeakerHigh size={20} /> : <SpeakerSlash size={20} />}
              </button>
            )}
          </div>
        </header>
        <main className="content">
          {!state ? (
            <p className="muted">Verbinde mit dem Server …</p>
          ) : page === 'live' ? (
            <Live server={server} state={state} stats={stats} onShowSessions={() => setPage('sessions')} />
          ) : page === 'sessions' ? (
            <Sessions changed={changed} state={state} />
          ) : page === 'pilots' ? (
            <Pilots state={state} stats={stats} />
          ) : (
            <SettingsPage state={state} />
          )}
        </main>
      </div>

      <nav className="bottom-nav">{nav}</nav>
    </div>
  )
}

/** /api/stats, fetched again when sessions or passes change.
 * @param {import('./types').State | null} state @param {number} changed */
function useStats(state, changed) {
  const [stats, setStats] = useState(/** @type {import('./types').Stats | null} */ (null))
  const activeId = state?.active?.session.id
  const passes = state?.active?.passes.length
  useEffect(() => {
    api('/stats').then(setStats).catch(() => {})
  }, [activeId, passes, changed])
  return stats
}
