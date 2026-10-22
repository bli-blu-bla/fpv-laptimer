// @ts-check
// The frame: navigation (a sidebar on wide screens, a tab bar at the bottom
// on phones), the top bar, and the pages. "?view=zuschauer" shows only the
// spectator view, full screen.

import { useEffect, useState } from 'react'
import {
  ArrowSquareOut,
  ClockCounterClockwise,
  FlagCheckered,
  MonitorPlay,
  SlidersHorizontal,
  SpeakerHigh,
  SpeakerSlash,
  Timer,
  Trophy,
  UsersThree,
} from '@phosphor-icons/react'
import { api, saveSettings, useServer } from './api.js'
import Best from './pages/Best.jsx'
import Control from './pages/Control.jsx'
import Pilots from './pages/Pilots.jsx'
import Sessions from './pages/Sessions.jsx'
import SettingsPage from './pages/Settings.jsx'
import Watch from './pages/Watch.jsx'

/** [id, title, short label for the phone's tab bar, icon] */
const PAGES = /** @type {const} */ ([
  ['control', 'Rennleitung', 'Leitung', FlagCheckered],
  ['watch', 'Zuschauer', 'Zuschauer', MonitorPlay],
  ['best', 'Bestzeiten', 'Bestzeiten', Trophy],
  ['sessions', 'Sessions', 'Sessions', ClockCounterClockwise],
  ['pilots', 'Piloten', 'Piloten', UsersThree],
  ['settings', 'Einstellungen', 'Setup', SlidersHorizontal],
])

const FULL_SCREEN_WATCH = new URLSearchParams(location.search).get('view') === 'zuschauer'

export default function App() {
  const server = useServer()
  const { state, connected, changed } = server
  const [page, setPage] = useState(/** @type {(typeof PAGES)[number][0]} */ ('control'))
  const stats = useStats(state, changed)

  const boards = state?.boards ?? []
  const failed = boards.filter((b) => b.failed).length
  const lost = boards.reduce((n, b) => n + (b.stats?.lost ?? 0), 0)
  const ok = connected && boards.length > 0 && !failed
  const boardText = !connected ? 'Keine Verbindung' : `${boards.length} Board${boards.length === 1 ? '' : 's'}${failed ? `, ${failed} ausgefallen` : ''}`
  const raceCount = `Heute · ${stats?.today_races ?? 0} Rennen`

  if (FULL_SCREEN_WATCH) {
    return (
      <div className="main">
        <header className="topbar">
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <div className="brand-icon">
              <Timer size={18} />
            </div>
            <span className="kicker">{raceCount}</span>
          </div>
          {!connected && <span className="right error">Keine Verbindung</span>}
        </header>
        <main className="content">{state ? <Watch server={server} state={state} /> : <p className="muted">Verbinde …</p>}</main>
      </div>
    )
  }

  /** @param {boolean} short */
  const nav = (short) =>
    PAGES.map(([id, title, label, Icon]) => (
      <button key={id} className={page === id ? 'nav-btn on' : 'nav-btn'} onClick={() => setPage(id)}>
        <Icon size={22} />
        <span>{short ? label : title}</span>
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
        {nav(false)}
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
            <span className="kicker">{raceCount}</span>
            <span className="title">{PAGES.find((p) => p[0] === page)?.[1]}</span>
          </div>
          <div className="right">
            <span className="narrow-only" style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <span className={ok ? 'status-dot' : 'status-dot bad'} />
              {boardText}
            </span>
            {page === 'watch' && (
              <a className="btn btn-ghost btn-icon" href="?view=zuschauer" target="_blank" title="Als Vollbild in neuem Fenster (für Fernseher/Tablet)">
                <ArrowSquareOut size={20} />
              </a>
            )}
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
          ) : page === 'control' ? (
            <Control server={server} state={state} stats={stats} />
          ) : page === 'watch' ? (
            <Watch server={server} state={state} />
          ) : page === 'best' ? (
            <Best state={state} stats={stats} />
          ) : page === 'sessions' ? (
            <Sessions changed={changed} state={state} />
          ) : page === 'pilots' ? (
            <Pilots state={state} stats={stats} />
          ) : (
            <SettingsPage state={state} live={server.live} />
          )}
        </main>
      </div>

      <nav className="bottom-nav">{nav(true)}</nav>
    </div>
  )
}

/** /api/stats, fetched again when sessions or passes change.
 * @param {import('./types').State | null} state @param {number} changed */
function useStats(state, changed) {
  const [stats, setStats] = useState(/** @type {import('./types').Stats | null} */ (null))
  const activeId = state?.active?.session.id
  const passes = state?.active?.passes.length
  const consecutive = state?.settings.consecutive
  useEffect(() => {
    api('/stats').then(setStats).catch(() => {})
  }, [activeId, passes, changed, consecutive])
  return stats
}
