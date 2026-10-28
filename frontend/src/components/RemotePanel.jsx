// @ts-check
// API & Hooks → Server: the connection to a server of the user's
// (server/src/remote.rs). The laptimer connects itself to a WebSocket URL
// and sends what the API's WebSocket sends (hello with the state, then every
// event); the server may send commands too, unless switched off. Two
// optional ways to authenticate: an Authorization header in the handshake,
// and a token sent as the first message ({"type": "auth", "token": …}).
// Neither comes back from the server, only whether it is set.

import { useEffect, useState } from 'react'
import { PlugsConnectedIcon } from '@phosphor-icons/react'
import { api } from '../api.js'
import { dateTime } from '../format.js'

/** How often the status is fetched while the panel is open, ms. */
const POLL_MS = 2000

/** @type {Record<import('../types').RemoteStatus['state'], string>} */
const STATE_TEXT = {
  off: 'Aus',
  connecting: 'Verbinde …',
  connected: 'Verbunden',
  waiting: 'Getrennt, nächster Versuch gleich',
}

export default function RemotePanel() {
  const [view, setView] = useState(/** @type {import('../types').RemoteView | null} */ (null))
  // The form; a secret null: leave the saved one as it is ("" removes it).
  const [draft, setDraft] = useState(/** @type {Draft | null} */ (null))
  const [error, setError] = useState('')
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    let stop = false
    const load = () =>
      api('/remote')
        .then((v) => {
          if (stop) return
          setView(v)
          // The form starts from the saved settings, once.
          setDraft((d) => d ?? fresh(v))
        })
        .catch((e) => !stop && setError(e.message))
    load()
    const timer = setInterval(load, POLL_MS)
    return () => {
      stop = true
      clearInterval(timer)
    }
  }, [])

  if (!view || !draft) {
    return (
      <Panel>
        {error ? <p className="error">{error}</p> : <span className="muted small">Lade …</span>}
      </Panel>
    )
  }

  /** @param {Partial<typeof draft>} patch */
  const set = (patch) => setDraft({ ...draft, ...patch })
  const dirty =
    draft.enabled !== view.enabled ||
    draft.url.trim() !== view.url ||
    draft.commands !== view.commands ||
    draft.authorization !== null ||
    draft.auth_token !== null

  async function save() {
    if (!draft) return
    setSaving(true)
    setError('')
    try {
      /** @type {Record<string, any>} */
      const body = { enabled: draft.enabled, url: draft.url.trim(), commands: draft.commands }
      if (draft.authorization !== null) body.authorization = draft.authorization
      if (draft.auth_token !== null) body.auth_token = draft.auth_token
      const v = await api('/remote', { method: 'PUT', body })
      setView(v)
      setDraft(fresh(v))
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    } finally {
      setSaving(false)
    }
  }

  const st = view.status
  return (
    <Panel>
      <span className="muted small">
        Der Laptimer verbindet sich selbst mit einem WebSocket-Server und schickt ihm dasselbe wie die API: erst den Zustand, dann jedes Ereignis
        live. Für einen Server, der immer läuft, während der Timer nur am Platz an ist. Kurze Abbrüche überbrückt er (Ereignisse der letzten 5
        Minuten werden nachgeschickt).
      </span>

      <button className="row-toggle" onClick={() => set({ enabled: !draft.enabled })}>
        <span className="text">
          <span>Verbindung aktiv</span>
        </span>
        <span className={draft.enabled ? 'switch on' : 'switch'} />
      </button>
      <label className="field">
        <span>Adresse</span>
        <input
          className="input mono"
          value={draft.url}
          onChange={(e) => set({ url: e.target.value })}
          placeholder="wss://server.example.org/laptimer"
          spellCheck={false}
          autoCapitalize="off"
        />
      </label>
      <Secret
        label="Authorization-Header (beim Verbinden)"
        placeholder="z. B. Bearer abc123"
        isSet={view.authorization_set}
        value={draft.authorization}
        onChange={(authorization) => set({ authorization })}
      />
      <Secret
        label="Token als erste Nachricht"
        hint={'Vor allem anderen: {"type": "auth", "token": "…"}'}
        placeholder="optional"
        isSet={view.auth_token_set}
        value={draft.auth_token}
        onChange={(auth_token) => set({ auth_token })}
      />
      <button className="row-toggle" onClick={() => set({ commands: !draft.commands })}>
        <span className="text">
          <span>Befehle vom Server erlauben</span>
          <span className="hint">Der Server darf alles, was ein API-Schlüssel darf (Sessions starten, Piloten anlegen, …)</span>
        </span>
        <span className={draft.commands ? 'switch on' : 'switch'} />
      </button>

      <div className="inset" style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span className={st.state === 'connected' ? 'status-dot' : st.state === 'off' ? 'status-dot off' : 'status-dot bad'} />
          {STATE_TEXT[st.state]}
          {st.since_us && st.state !== 'off' && <span className="muted small">seit {dateTime(st.since_us).replace(/^Heute /, '')}</span>}
        </span>
        {st.error && st.state !== 'connected' && <span className="small error">{st.error}</span>}
        {st.state !== 'off' && (
          <span className="muted small">
            {st.sent} gesendet{st.buffered ? ` · ${st.buffered} warten auf die Verbindung` : ''}
            {st.dropped ? ` · ${st.dropped} verworfen` : ''}
          </span>
        )}
      </div>

      {error && <p className="error">{error}</p>}
      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        {dirty && (
          <button className="btn btn-ghost" onClick={() => setDraft(fresh(view))}>
            Verwerfen
          </button>
        )}
        <button className="btn btn-primary" disabled={!dirty || saving} onClick={save}>
          Speichern
        </button>
      </div>
    </Panel>
  )
}

/** @typedef {{enabled: boolean, url: string, authorization: string | null, auth_token: string | null, commands: boolean}} Draft */

/** The form for the saved settings (secrets untouched). @param {import('../types').RemoteView} v @returns {Draft} */
const fresh = (v) => ({ enabled: v.enabled, url: v.url, authorization: null, auth_token: null, commands: v.commands })

/** A secret: never shown again; empty while set keeps it, "Entfernen" clears it.
 * @param {{label: string, hint?: string, placeholder: string, isSet: boolean, value: string | null, onChange: (v: string | null) => void}} props */
function Secret({ label, hint, placeholder, isSet, value, onChange }) {
  const removing = isSet && value === ''
  return (
    <label className="field">
      <span>{label}</span>
      {hint && <span className="muted small mono">{hint}</span>}
      <span style={{ display: 'flex', gap: 8 }}>
        <input
          className="input mono"
          type="password"
          value={value ?? ''}
          onChange={(e) => onChange(e.target.value === '' ? null : e.target.value)}
          placeholder={removing ? 'wird entfernt' : isSet ? 'gesetzt (leer lassen = behalten)' : placeholder}
          autoComplete="off"
        />
        {isSet && !removing && (
          <button type="button" className="btn btn-ghost" onClick={(e) => (e.preventDefault(), onChange(''))}>
            Entfernen
          </button>
        )}
      </span>
    </label>
  )
}

/** @param {{children: React.ReactNode}} props */
function Panel({ children }) {
  return (
    <div className="panel">
      <div className="panel-head">
        <PlugsConnectedIcon size={18} /> Server-Verbindung
      </div>
      <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {children}
      </div>
    </div>
  )
}
