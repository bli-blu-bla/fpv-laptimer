// @ts-check
// The public API for other programs (server/src/api.rs, docs/API.md): keys,
// a reference of its commands and events, a tester (HTTP and WebSocket, with
// the live events), and the hooks (scripts run on events, server/src/hooks.rs).

import { useEffect, useRef, useState } from 'react'
import { BookOpen, Copy, Key, Lightning, PaperPlaneTilt, PencilSimple, Play, Plug, PlugsConnected, Plus, Trash } from '@phosphor-icons/react'
import { api } from '../api.js'
import { COMMANDS, EVENTS, restCall } from '../apiCatalog.js'
import { dateTime } from '../format.js'

/** The key the tester uses, kept in this browser. */
const KEY_STORE = 'laptimer.apiKey'
const TABS = /** @type {const} */ ([
  ['keys', 'Schlüssel', Key],
  ['docs', 'Doku', BookOpen],
  ['test', 'Testen', PaperPlaneTilt],
  ['hooks', 'Hooks', Lightning],
])

const base = `${location.protocol}//${location.host}/api/v1`
const wsBase = `${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/api/v1/ws`

/** @param {string} k */
function storedKey(k) {
  try {
    return localStorage.getItem(k) ?? ''
  } catch {
    return ''
  }
}
/** @param {string} value */
function storeKey(value) {
  try {
    localStorage.setItem(KEY_STORE, value)
  } catch {
    // private mode: the key just isn't remembered
  }
}

/** Copies text; the Clipboard API needs HTTPS or localhost, the old way works over plain HTTP. @param {string} text */
async function copy(text) {
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    const t = document.createElement('textarea')
    t.value = text
    document.body.appendChild(t)
    t.select()
    document.execCommand('copy')
    t.remove()
  }
}

export default function ApiPage() {
  const [tab, setTab] = useState(/** @type {(typeof TABS)[number][0]} */ ('keys'))
  const [key, setKey] = useState(() => storedKey(KEY_STORE))
  /** @param {string} k */
  const useKey = (k) => {
    setKey(k)
    storeKey(k)
  }
  return (
    <>
      <div className="seg" style={{ maxWidth: 560 }}>
        {TABS.map(([id, label, Icon]) => (
          <button key={id} className={tab === id ? 'on' : ''} onClick={() => setTab(id)}>
            <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
              <Icon size={16} /> {label}
            </span>
          </button>
        ))}
      </div>
      {tab === 'keys' && <Keys onKey={useKey} />}
      {tab === 'docs' && <Docs />}
      {tab === 'test' && <Tester apiKey={key} setKey={useKey} />}
      {tab === 'hooks' && <Hooks />}
    </>
  )
}

// --- keys ---

/** @typedef {{id: number, name: string, prefix: string, created_us: number, last_used_us: number | null}} ApiKey */

/** @param {{onKey: (key: string) => void}} props */
function Keys({ onKey }) {
  const [keys, setKeys] = useState(/** @type {ApiKey[] | null} */ (null))
  const [name, setName] = useState('')
  const [fresh, setFresh] = useState(/** @type {{name: string, key: string} | null} */ (null))
  const [confirm, setConfirm] = useState(/** @type {number | null} */ (null))
  const [error, setError] = useState('')
  const load = () =>
    api('/keys')
      .then(setKeys)
      .catch((e) => setError(e.message))
  useEffect(() => {
    load()
  }, [])

  /** @param {React.FormEvent} e */
  async function create(e) {
    e.preventDefault()
    setError('')
    try {
      const r = await api('/keys', { body: { name } })
      setFresh({ name: name.trim(), key: r.key })
      onKey(r.key)
      setName('')
      load()
    } catch (err) {
      setError(/** @type {Error} */ (err).message)
    }
  }
  /** @param {number} id */
  async function remove(id) {
    await api(`/keys/${id}`, { method: 'DELETE' }).catch((e) => setError(e.message))
    setConfirm(null)
    load()
  }

  return (
    <div className="two-col" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 420px), 1fr))' }}>
      <div className="panel">
        <div className="panel-head">
          <Key size={18} /> API-Schlüssel
          <span className="end muted small">{keys?.length ?? 0}</span>
        </div>
        <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <form onSubmit={create} style={{ display: 'flex', gap: 8 }}>
            <input className="input" value={name} onChange={(e) => setName(e.target.value)} placeholder="Wofür? z. B. Stream-Overlay" />
            <button className="btn btn-primary" disabled={!name.trim()}>
              Anlegen
            </button>
          </form>
          {fresh && (
            <div className="key-box">
              <span className="small">
                Schlüssel für <b>{fresh.name}</b>: jetzt kopieren, er wird nur dieses eine Mal angezeigt.
              </span>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <code className="code-line" style={{ flex: 1 }}>
                  {fresh.key}
                </code>
                <button className="btn btn-secondary btn-icon" title="Kopieren" onClick={() => copy(fresh.key)}>
                  <Copy size={18} />
                </button>
              </div>
              <span className="muted small">Der Tester (Reiter „Testen“) verwendet ihn schon.</span>
            </div>
          )}
          {error && <p className="error">{error}</p>}
        </div>
        {keys && !keys.length && <div className="empty">Noch keine Schlüssel.</div>}
        {keys?.map((k) => (
          <div key={k.id} className="list-row">
            <span style={{ display: 'flex', flexDirection: 'column', minWidth: 0, flex: 1, padding: '6px 0' }}>
              <span style={{ fontWeight: 600 }}>{k.name}</span>
              <span className="muted small">
                <code>{k.prefix}…</code> · angelegt {dateTime(k.created_us)} · {k.last_used_us ? `zuletzt ${dateTime(k.last_used_us)}` : 'nie benutzt'}
              </span>
            </span>
            {confirm === k.id ? (
              <button className="btn btn-danger" onClick={() => remove(k.id)}>
                Wirklich löschen
              </button>
            ) : (
              <button className="btn btn-ghost btn-icon" title="Löschen (Programme mit diesem Schlüssel verlieren den Zugang)" onClick={() => setConfirm(k.id)}>
                <Trash size={18} />
              </button>
            )}
          </div>
        ))}
      </div>

      <div className="panel">
        <div className="panel-head">
          <Plug size={18} /> So verbindet sich ein Programm
        </div>
        <div className="panel-body doc" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          <span className="small">HTTP: jeder Befehl als Anfrage, mit dem Schlüssel im Header.</span>
          <pre className="code">{`curl -H "Authorization: Bearer lt_…" \\
  ${base}/state

curl -H "Authorization: Bearer lt_…" \\
  -H "content-type: application/json" \\
  -d '{"race": true, "laps": 3}' \\
  ${base}/session/start`}</pre>
          <span className="small">WebSocket: alle Events live, Befehle über dieselbe Verbindung.</span>
          <pre className="code">{`${wsBase}?key=lt_…
→ {"type":"hello", "state": {…}}
← {"id":1, "cmd":"session.start", "args":{"race":true}}
→ {"type":"reply", "id":1, "ok":true, "data":{…}}
→ {"type":"event", "event":"pilot_pass", "data":{…}}`}</pre>
          <span className="muted small">
            Die Weboberfläche selbst braucht keinen Schlüssel (wie bisher): wer im Netz die Seite öffnen kann, kann auch Schlüssel anlegen.
          </span>
        </div>
      </div>
    </div>
  )
}

// --- reference ---

function Docs() {
  const groups = [...new Set(COMMANDS.map((c) => c.group))]
  return (
    <div className="two-col" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 460px), 1fr))' }}>
      <div className="panel">
        <div className="panel-head">
          <BookOpen size={18} /> Befehle
          <span className="end muted small">REST unter /api/v1 · WebSocket: cmd</span>
        </div>
        <div className="panel-body doc">
          <p className="muted small" style={{ marginTop: 0 }}>
            Jeder Befehl geht als REST-Anfrage, als <code>POST /api/v1/cmd/&lt;cmd&gt;</code> mit den Argumenten als JSON, oder über den WebSocket. Fehler:
            Status 400 mit <code>{'{"error": "…"}'}</code> (deutsch, zum Anzeigen); ohne gültigen Schlüssel 401.
          </p>
          {groups.map((g) => (
            <div key={g}>
              <div className="kicker" style={{ margin: '14px 0 6px' }}>
                {g}
              </div>
              {COMMANDS.filter((c) => c.group === g).map((c) => (
                <div key={c.cmd} className="doc-item">
                  <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, alignItems: 'baseline' }}>
                    <span className={'method m-' + c.method.toLowerCase()}>{c.method}</span>
                    <code>{c.path}</code>
                    <code className="muted">{c.cmd}</code>
                  </div>
                  <div style={{ fontWeight: 500 }}>{c.title}</div>
                  {c.desc && <div className="muted small">{c.desc}</div>}
                  {c.args && (
                    <ul className="args">
                      {c.args.map(([n, d]) => (
                        <li key={n}>
                          <code>{n}</code> <span className="muted">{d}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              ))}
            </div>
          ))}
        </div>
      </div>
      <div className="panel">
        <div className="panel-head">
          <Lightning size={18} /> Events
          <span className="end muted small">WebSocket und Hooks</span>
        </div>
        <div className="panel-body doc">
          <pre className="code">{`{"type": "event", "event": "pilot_pass", "seq": 17,
 "time_us": 1792697215501133,
 "data": {"pilot": "NITRO", "channel": 0, "kind": "lap",
          "lap": 2, "lap_time_s": 13.712, …}}`}</pre>
          <p className="muted small">
            Zeiten in µs (Wanduhr seit 1970, Dauern als <code>…_us</code>, gerundet als <code>…_s</code>). <code>seq</code> zählt ab Serverstart; eine Lücke
            heißt: Events verpasst (dann <code>state</code> neu holen).
          </p>
          {EVENTS.map((e) => (
            <div key={e.name} className="doc-item">
              <code style={{ fontWeight: 600 }}>{e.name}</code>
              <div className="small">{e.desc}</div>
              <div className="muted small">{e.fields}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

// --- tester ---

/** @typedef {{dir: 'in' | 'out' | 'info', head: string, text: string, at: number}} LogLine */

/** @param {{apiKey: string, setKey: (k: string) => void}} props */
function Tester({ apiKey, setKey }) {
  const [cmd, setCmd] = useState('state')
  const command = COMMANDS.find((c) => c.cmd === cmd) ?? COMMANDS[0]
  const [argsText, setArgsText] = useState('{}')
  const [via, setVia] = useState(/** @type {'http' | 'ws'} */ ('http'))
  const [result, setResult] = useState(/** @type {{status: string, ms: number, text: string} | null} */ (null))
  const ws = useApiSocket(apiKey)

  /** @param {string} name */
  function pick(name) {
    setCmd(name)
    const c = COMMANDS.find((x) => x.cmd === name)
    setArgsText(JSON.stringify(c?.example ?? {}, null, 2))
    setResult(null)
  }

  /** @type {Record<string, unknown> | null} */
  let args = null
  try {
    const parsed = JSON.parse(argsText || '{}')
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) args = parsed
  } catch {
    // shown below the field
  }

  async function send() {
    if (!args) return
    const t0 = performance.now()
    if (via === 'ws') {
      const reply = await ws.send(cmd, args)
      setResult({ status: reply.ok ? 'ok' : 'Fehler', ms: performance.now() - t0, text: JSON.stringify(reply.ok ? reply.data : reply.error, null, 2) })
      return
    }
    const { path, body, hasBody } = restCall(command, args)
    try {
      const res = await fetch(base + path, {
        method: command.method,
        headers: { authorization: `Bearer ${apiKey}`, ...(hasBody ? { 'content-type': 'application/json' } : {}) },
        body: hasBody ? JSON.stringify(body) : undefined,
      })
      const text = await res.text()
      let pretty = text
      try {
        pretty = JSON.stringify(JSON.parse(text), null, 2)
      } catch {
        // not JSON: as it came
      }
      setResult({ status: `${res.status} ${res.statusText}`, ms: performance.now() - t0, text: pretty })
    } catch (e) {
      setResult({ status: 'Netzwerkfehler', ms: performance.now() - t0, text: String(e) })
    }
  }

  const call = args ? restCall(command, args) : null
  const curl = call
    ? `curl -X ${command.method} -H "Authorization: Bearer ${apiKey || 'lt_…'}"` +
      (call.hasBody ? ` \\\n  -H "content-type: application/json" -d '${JSON.stringify(call.body)}'` : '') +
      ` \\\n  ${base}${call.path}`
    : ''
  const wsLine = args ? JSON.stringify({ id: 1, cmd, args }) : ''

  return (
    <div className="two-col" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 440px), 1fr))' }}>
      <div className="panel">
        <div className="panel-head">
          <PaperPlaneTilt size={18} /> Anfrage
        </div>
        <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <label className="field">
            <span>API-Schlüssel</span>
            <input className="input mono" value={apiKey} onChange={(e) => setKey(e.target.value.trim())} placeholder="lt_… (unter „Schlüssel“ anlegen)" />
          </label>
          <label className="field">
            <span>Befehl</span>
            <select className="input" value={cmd} onChange={(e) => pick(e.target.value)}>
              {COMMANDS.map((c) => (
                <option key={c.cmd} value={c.cmd}>
                  {c.cmd} – {c.title}
                </option>
              ))}
            </select>
          </label>
          {command.desc && <span className="muted small">{command.desc}</span>}
          {command.args && (
            <ul className="args small" style={{ margin: 0 }}>
              {command.args.map(([n, d]) => (
                <li key={n}>
                  <code>{n}</code> <span className="muted">{d}</span>
                </li>
              ))}
            </ul>
          )}
          <label className="field">
            <span>Argumente (JSON)</span>
            <textarea
              className="input mono"
              rows={Math.min(10, argsText.split('\n').length + 1)}
              value={argsText}
              onChange={(e) => setArgsText(e.target.value)}
              spellCheck={false}
            />
          </label>
          {!args && <span className="error small">Kein gültiges JSON-Objekt.</span>}
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
            <div className="seg" style={{ flex: 'none' }}>
              <button className={via === 'http' ? 'on' : ''} onClick={() => setVia('http')}>
                HTTP
              </button>
              <button className={via === 'ws' ? 'on' : ''} onClick={() => setVia('ws')}>
                WebSocket
              </button>
            </div>
            <button className="btn btn-primary" style={{ marginLeft: 'auto' }} disabled={!args || !apiKey || (via === 'ws' && !ws.connected)} onClick={send}>
              <Play size={16} /> Senden
            </button>
          </div>
          {via === 'ws' && !ws.connected && <span className="muted small">Erst rechts den WebSocket verbinden.</span>}
          <pre className="code">{via === 'http' ? curl : wsLine}</pre>
          {result && (
            <>
              <span className="small">
                <b className={result.status.startsWith('2') || result.status === 'ok' ? 'ok-color' : 'error'}>{result.status}</b>{' '}
                <span className="muted">· {Math.round(result.ms)} ms</span>
              </span>
              <pre className="code result">{result.text}</pre>
            </>
          )}
        </div>
      </div>

      <div className="panel">
        <div className="panel-head">
          <PlugsConnected size={18} /> WebSocket · Events
          <span className="end" style={{ display: 'flex', gap: 8 }}>
            {ws.log.length > 0 && (
              <button className="btn btn-ghost" onClick={ws.clear}>
                Leeren
              </button>
            )}
            <button className={ws.connected ? 'btn btn-secondary' : 'btn btn-primary'} disabled={!apiKey} onClick={ws.connected ? ws.disconnect : ws.connect}>
              {ws.connected ? 'Trennen' : 'Verbinden'}
            </button>
          </span>
        </div>
        <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          {!ws.log.length && <span className="muted small">Verbinden, dann z. B. ein Training starten: hier erscheint jedes Event.</span>}
          <div className="ws-log">
            {ws.log.map((l, i) => (
              <div key={i} className={'ws-line ' + l.dir}>
                <span className="muted tnum">{new Date(l.at).toLocaleTimeString('de-DE')}</span>
                <span className="ws-dir">{l.dir === 'in' ? '→' : l.dir === 'out' ? '←' : '·'}</span>
                <span style={{ minWidth: 0 }}>
                  <b className="ws-head">{l.head}</b> <code>{l.text}</code>
                </span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}

/** A WebSocket to /api/v1/ws for the tester: a log of everything, and
 * commands whose reply comes back as a promise. @param {string} apiKey */
function useApiSocket(apiKey) {
  const [connected, setConnected] = useState(false)
  const [log, setLog] = useState(/** @type {LogLine[]} */ ([]))
  const sock = useRef(/** @type {WebSocket | null} */ (null))
  const waiting = useRef(/** @type {Map<number, (reply: any) => void>} */ (new Map()))
  const nextId = useRef(1)

  /** @param {LogLine['dir']} dir @param {string} head @param {string} [text] */
  const add = (dir, head, text = '') => setLog((l) => [{ dir, head, text, at: Date.now() }, ...l].slice(0, 200))

  function connect() {
    sock.current?.close()
    const s = new WebSocket(`${wsBase}?key=${encodeURIComponent(apiKey)}`)
    sock.current = s
    add('info', 'verbinde …')
    s.onopen = () => setConnected(true)
    s.onclose = (e) => {
      setConnected(false)
      // A refused key ends the handshake: the browser only sees code 1006.
      add('info', `getrennt (${e.code}${e.code === 1006 ? ', Schlüssel ungültig oder Server weg' : ''})`)
    }
    s.onmessage = (ev) => {
      const msg = JSON.parse(ev.data)
      if (msg.type === 'reply') {
        waiting.current.get(msg.id)?.(msg)
        waiting.current.delete(msg.id)
        add('in', `Antwort #${msg.id} ${msg.ok ? 'ok' : 'Fehler'}`, JSON.stringify(msg.ok ? msg.data : msg.error))
      } else if (msg.type === 'event') add('in', msg.event, JSON.stringify(msg.data))
      else if (msg.type === 'hello') add('in', 'hello', `api ${msg.api}, ${msg.commands?.length} Befehle, state`)
      else add('in', msg.type ?? '?', ev.data)
    }
  }
  useEffect(() => () => sock.current?.close(), [])

  return {
    connected,
    log,
    connect,
    disconnect: () => sock.current?.close(),
    clear: () => setLog([]),
    /** @param {string} cmd @param {Record<string, unknown>} args @returns {Promise<any>} */
    send(cmd, args) {
      const id = nextId.current++
      const text = JSON.stringify({ id, cmd, args })
      add('out', cmd, JSON.stringify(args))
      sock.current?.send(text)
      return new Promise((resolve) => waiting.current.set(id, resolve))
    },
  }
}

// --- hooks ---

/** @typedef {{id: number, event: string, script: string, args: string, background: boolean, enabled: boolean}} Hook */
/** @typedef {{time_us: number, event: string, hook_id: number, command: string, background: boolean, exit_code: number | null, duration_ms: number, error: string | null}} HookRun */
/** @typedef {{dir: string, scripts: string[], hooks: Hook[], runs: HookRun[], events: string[]}} HookInfo */

/** @param {string} event */
const eventLabel = (event) => (event === '*' ? '* (jedes Event)' : event)

function Hooks() {
  const [info, setInfo] = useState(/** @type {HookInfo | null} */ (null))
  /** The hook in the form: new (id 0) or being changed; null: no form. */
  const [draft, setDraft] = useState(/** @type {Hook | null} */ (null))
  const [confirm, setConfirm] = useState(/** @type {number | null} */ (null))
  const [error, setError] = useState('')
  const load = () =>
    api('/hooks')
      .then(setInfo)
      .catch(() => {})
  useEffect(() => {
    load()
    const id = setInterval(load, 2000) // runs and new scripts show up by themselves
    return () => clearInterval(id)
  }, [])

  /** Sends a request, then reloads; errors show above the list. @param {string} path @param {{method?: string, body?: unknown}} [opts] */
  async function send(path, opts) {
    setError('')
    try {
      await api(path, opts)
      await load()
      return true
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
      return false
    }
  }
  async function saveDraft() {
    if (!draft) return
    const { id, ...hook } = draft
    const ok = id ? await send(`/hooks/${id}`, { method: 'PUT', body: hook }) : await send('/hooks', { body: hook })
    if (ok) setDraft(null)
  }
  /** @param {Hook} h */
  const toggle = (h) => send(`/hooks/${h.id}`, { method: 'PUT', body: { ...h, enabled: !h.enabled } })
  /** @param {number} id */
  async function remove(id) {
    await send(`/hooks/${id}`, { method: 'DELETE' })
    setConfirm(null)
  }

  if (!info) return <p className="muted">Lade …</p>
  const fresh = () => setDraft({ id: 0, event: 'start_signal', script: info.scripts[0] ?? '', args: '', background: false, enabled: true })
  return (
    <div className="two-col" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 440px), 1fr))' }}>
      <div className="panel">
        <div className="panel-head">
          <Lightning size={18} /> Hooks
          {!draft && (
            <button className="btn btn-ghost end" onClick={fresh}>
              <Plus size={16} /> Hinzufügen
            </button>
          )}
        </div>
        <div className="panel-body doc" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          <span className="small">
            Bei einem Event startet der Server die passenden Scripts, nacheinander. Scripts liegen im Ordner <code>{info.dir}</code>; wählbar sind die
            ausführbaren Dateien darin (<code>chmod +x</code>).
          </span>
          {draft && <HookForm draft={draft} setDraft={setDraft} info={info} onSave={saveDraft} />}
          {error && <p className="error">{error}</p>}
        </div>
        {!info.hooks.length && !draft && <div className="empty">Noch keine Hooks.</div>}
        {info.hooks.map((h) => (
          <div key={h.id} className="list-row" style={{ opacity: h.enabled ? 1 : 0.55 }}>
            <button className="btn btn-ghost btn-icon" title={h.enabled ? 'Ausschalten' : 'Einschalten'} onClick={() => toggle(h)}>
              <span className={h.enabled ? 'switch on' : 'switch'} />
            </button>
            <span style={{ display: 'flex', flexDirection: 'column', minWidth: 0, flex: 1, padding: '6px 0' }}>
              <code style={{ fontWeight: 600 }}>{eventLabel(h.event)}</code>
              <code className="muted small hook-cmd">
                {h.script} {h.args}
              </code>
              {(h.background || !info.scripts.includes(h.script)) && (
                <span className="small">
                  {h.background && <span className="muted">im Hintergrund </span>}
                  {!info.scripts.includes(h.script) && <span className="error">Script fehlt oder ist nicht ausführbar</span>}
                </span>
              )}
            </span>
            {confirm === h.id ? (
              <button className="btn btn-danger" onClick={() => remove(h.id)}>
                Wirklich löschen
              </button>
            ) : (
              <span style={{ display: 'flex' }}>
                <button
                  className="btn btn-ghost btn-icon"
                  title="Jetzt einmal ausführen (LAPTIMER_TEST=1)"
                  onClick={() => send(`/hooks/${h.id}/test`, { method: 'POST' })}
                >
                  <Play size={18} />
                </button>
                <button className="btn btn-ghost btn-icon" title="Bearbeiten" onClick={() => setDraft({ ...h })}>
                  <PencilSimple size={18} />
                </button>
                <button className="btn btn-ghost btn-icon" title="Löschen" onClick={() => setConfirm(h.id)}>
                  <Trash size={18} />
                </button>
              </span>
            )}
          </div>
        ))}
        <div className="panel-body doc">
          <span className="muted small">
            Umgebung: <code>LAPTIMER_EVENT</code>, <code>LAPTIMER_SEQ</code>, <code>LAPTIMER_TIME_US</code>, <code>LAPTIMER_JSON</code> (alles) und jedes
            einfache Feld der Daten als <code>LAPTIMER_&lt;FELD&gt;</code> (z. B. <code>LAPTIMER_PILOT</code>, <code>LAPTIMER_LAP</code>,{' '}
            <code>LAPTIMER_LAP_TIME_S</code>; true/false als 1/0). <Play size={12} /> startet einen Hook mit den Daten des letzten echten Events und{' '}
            <code>LAPTIMER_TEST=1</code>. Ein Hook darf 10 s laufen; Längeres (Blinken) im Hintergrund.
          </span>
        </div>
      </div>
      <div className="panel">
        <div className="panel-head">
          Letzte Läufe
          <span className="end muted small">{info.runs.length}</span>
        </div>
        {!info.runs.length && <div className="empty">Noch kein Hook gelaufen.</div>}
        <div style={{ maxHeight: 640, overflowY: 'auto' }}>
          {info.runs.map((r, i) => (
            <div key={i} className="list-row" style={{ alignItems: 'flex-start', padding: '8px 16px' }}>
              <span className={r.error || r.exit_code !== 0 ? 'status-dot bad' : 'status-dot'} style={{ marginTop: 7 }} />
              <span style={{ display: 'flex', flexDirection: 'column', minWidth: 0, flex: 1 }}>
                <span>
                  <code style={{ fontWeight: 600 }}>{r.event}</code>{' '}
                  <span className="muted small">
                    {new Date(r.time_us / 1000).toLocaleTimeString('de-DE')} · {r.duration_ms} ms
                    {r.exit_code != null && r.exit_code !== 0 ? ` · Exit ${r.exit_code}` : ''}
                  </span>
                </span>
                <code className="muted small hook-cmd">
                  {r.command}
                  {r.background ? ' (Hintergrund)' : ''}
                </code>
                {r.error && <span className="error small">{r.error}</span>}
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

/** The form for a new or changed hook. @param {{draft: Hook, setDraft: (h: Hook | null) => void, info: HookInfo, onSave: () => void}} props */
function HookForm({ draft, setDraft, info, onSave }) {
  /** @param {Partial<Hook>} patch */
  const set = (patch) => setDraft({ ...draft, ...patch })
  // A changed hook whose script is gone keeps it in the list (shown as missing).
  const scripts = info.scripts.includes(draft.script) || !draft.script ? info.scripts : [draft.script, ...info.scripts]
  return (
    <div className="key-box" style={{ gap: 10 }}>
      <label className="field">
        <span>Event</span>
        <select className="input" value={draft.event} onChange={(e) => set({ event: e.target.value })}>
          {['*', ...info.events].map((e) => (
            <option key={e} value={e}>
              {eventLabel(e)}
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>Script</span>
        {scripts.length ? (
          <select className="input" value={draft.script} onChange={(e) => set({ script: e.target.value })}>
            {scripts.map((name) => (
              <option key={name}>{name}</option>
            ))}
          </select>
        ) : (
          <span className="error small">Im Ordner liegt kein ausführbares Script.</span>
        )}
      </label>
      <label className="field">
        <span>Argumente (durch Leerzeichen getrennt, ohne Shell)</span>
        <input className="input" value={draft.args} onChange={(e) => set({ args: e.target.value })} placeholder="z. B. 17 on" />
      </label>
      <button className="row-toggle" onClick={() => set({ background: !draft.background })}>
        <span className="text">
          <span>Im Hintergrund</span>
          <span className="hint">Nicht warten, bis es fertig ist (ohne Zeitlimit); für Längeres wie Blinken</span>
        </span>
        <span className={draft.background ? 'switch on' : 'switch'} />
      </button>
      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        <button className="btn btn-ghost" onClick={() => setDraft(null)}>
          Abbrechen
        </button>
        <button className="btn btn-primary" disabled={!draft.script} onClick={onSave}>
          {draft.id ? 'Speichern' : 'Hinzufügen'}
        </button>
      </div>
    </div>
  )
}
