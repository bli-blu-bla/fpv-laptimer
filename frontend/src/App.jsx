// @ts-check
import { useState } from 'react'
import { useServer } from './api.js'
import Live from './pages/Live.jsx'
import Sessions from './pages/Sessions.jsx'
import SettingsPage from './pages/Settings.jsx'

const TABS = /** @type {const} */ ([
  ['live', 'Live'],
  ['sessions', 'Sessions'],
  ['settings', 'Einstellungen'],
])

export default function App() {
  const server = useServer()
  const [tab, setTab] = useState(/** @type {'live' | 'sessions' | 'settings'} */ ('live'))
  const { state, connected } = server
  const failed = state?.boards.filter((b) => b.failed).length ?? 0

  return (
    <>
      <header className="top">
        <nav>
          {TABS.map(([id, label]) => (
            <button key={id} className={tab === id ? 'tab active' : 'tab'} onClick={() => setTab(id)}>
              {label}
            </button>
          ))}
        </nav>
        <span className={connected ? 'dot ok' : 'dot bad'} title={connected ? 'verbunden' : 'keine Verbindung'} />
        {state && (
          <span className={state.boards.length && !failed ? 'muted' : 'warn'}>
            {state.boards.length} Board{state.boards.length === 1 ? '' : 's'}
            {failed ? `, ${failed} ausgefallen` : ''}
          </span>
        )}
      </header>
      <main>
        {!state ? (
          <p className="muted">Verbinde mit dem Server …</p>
        ) : tab === 'live' ? (
          <Live server={server} state={state} />
        ) : tab === 'sessions' ? (
          <Sessions changed={server.changed} state={state} />
        ) : (
          <SettingsPage state={state} />
        )}
      </main>
    </>
  )
}
