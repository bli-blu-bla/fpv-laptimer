// @ts-check
// The password of the race control. Opened from the top bar, from a locked
// page, or by a change the server refused (`auth.js`).

import { useState } from 'react'
import { login, showLogin } from '../auth.js'
import Dialog from './Dialog.jsx'

export default function LoginDialog() {
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  /** @param {React.FormEvent} e */
  async function submit(e) {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      await login(password)
    } catch (err) {
      setError(/** @type {Error} */ (err).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog title="Anmelden" onClose={() => showLogin(false)}>
      <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        <p className="muted small" style={{ margin: 0 }}>
          Zum Ändern (Rennleitung, Einstellungen, Piloten, …) das Passwort aus <code>~/.laptimer/config.toml</code>. Das Gerät bleibt 30 Tage angemeldet.
        </p>
        <input className="input" type="password" autoFocus autoComplete="current-password" placeholder="Passwort" value={password} onChange={(e) => setPassword(e.target.value)} />
        {error && <p className="error" style={{ margin: 0 }}>{error}</p>}
        <button type="submit" className="btn btn-primary" disabled={busy || !password}>
          {busy ? 'Prüfe …' : 'Anmelden'}
        </button>
      </form>
    </Dialog>
  )
}

/** In place of a page that is only for changing things.
 * @param {{what: string}} props */
export function Locked({ what }) {
  return (
    <div className="panel" style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 12, alignItems: 'flex-start' }}>
      <span className="muted">{what} gibt es nur mit Anmeldung.</span>
      <button className="btn btn-primary" onClick={() => showLogin(true)}>
        Anmelden
      </button>
    </div>
  )
}
