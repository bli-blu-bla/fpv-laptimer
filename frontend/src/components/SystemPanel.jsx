// @ts-check
// Settings → System: the version, updates (upload or from Forgejo), back to
// the previous version, restarting the laptimer, rebooting or shutting down
// the Pi (server/src/update.rs).
//
// An update has two steps: the server checks and unpacks the package
// ("staged"), the page shows the version, then "install" makes a backup,
// switches and restarts. Meanwhile the WebSocket reconnects; once the
// server reports the new version, the page reloads itself (new page files).

import { useEffect, useRef, useState } from 'react'
import { ArrowCounterClockwiseIcon, ArrowsClockwiseIcon, CloudArrowDownIcon, CpuIcon, PowerIcon, UploadSimpleIcon } from '@phosphor-icons/react'
import { api } from '../api.js'
import Dialog from './Dialog.jsx'

/** @type {Record<import('../types').VersionOrder, string>} */
const ORDER = { newer: 'neuer', same: 'läuft gerade', older: 'älter', unclear: 'andere Version' }

/** How long a restart may take before the page stops waiting, ms. */
const RESTART_TIMEOUT = 150_000

/**
 * @typedef {object} Confirm
 * @property {string} title
 * @property {string} text
 * @property {string} label
 * @property {() => Promise<void>} run
 */

/** @param {{version: string, running: boolean}} props  `version`: the server's, from the WebSocket */
export default function SystemPanel({ version, running }) {
  const [status, setStatus] = useState(/** @type {import('../types').UpdateStatus | null} */ (null))
  const [releases, setReleases] = useState(/** @type {import('../types').OnlineRelease[] | null} */ (null))
  const [busy, setBusy] = useState('')
  const [error, setError] = useState('')
  const [confirm, setConfirm] = useState(/** @type {Confirm | null} */ (null))
  // Waiting for the server to come back after an install, rollback or restart.
  const [restart, setRestart] = useState(/** @type {{from: string, at: number} | null} */ (null))
  const fileInput = useRef(/** @type {HTMLInputElement | null} */ (null))

  const load = () =>
    api('/update')
      .then(setStatus)
      .catch((e) => setError(e.message))
  useEffect(() => {
    load()
  }, [])

  // Back after a restart: another version (or the same after a plain
  // restart, noticed by the reconnect) → load the new page files.
  useEffect(() => {
    if (!restart) return
    if (version !== restart.from) location.reload()
    const timer = setInterval(() => {
      if (Date.now() - restart.at > RESTART_TIMEOUT) {
        setRestart(null)
        setError('Der Server meldet sich nicht zurück. Läuft er noch? (systemctl --user status laptimer)')
      } else {
        // Same version: reload once the server answers again after going away.
        fetch('/api/update').then((r) => r.ok && Date.now() - restart.at > 5000 && location.reload(), () => {})
      }
    }, 2000)
    return () => clearInterval(timer)
  }, [restart, version])

  /** Runs a request with a busy text; shows its error. @param {string} text @param {() => Promise<any>} req */
  async function run(text, req) {
    setBusy(text)
    setError('')
    try {
      return await req()
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    } finally {
      setBusy('')
    }
  }

  /** The package and its signature, picked together. @param {FileList | null} files */
  async function upload(files) {
    const list = [...(files ?? [])]
    if (fileInput.current) fileInput.current.value = '' // the same files again later
    const signature = list.find((f) => f.name.endsWith('.minisig'))
    const pkg = list.find((f) => f.name.endsWith('.tar.gz'))
    if (!pkg || !signature) {
      setError('Bitte beide Dateien zusammen wählen: laptimer-…-aarch64.tar.gz und laptimer-…-aarch64.tar.gz.minisig.')
      return
    }
    const form = new FormData()
    form.append('package', pkg)
    form.append('signature', signature)
    await run('Lade hoch und prüfe die Signatur …', () => api('/update/upload', { form }))
    load()
  }

  const search = () => run('Frage Forgejo …', () => api('/update/releases').then((r) => setReleases(r.releases)))
  /** @param {string} tag */
  const download = async (tag) => {
    await run(`Lade ${tag} herunter und prüfe die Signatur …`, () => api('/update/download', { body: { tag } }))
    load()
  }
  const discard = () => run('', () => api('/update/staged', { method: 'DELETE' }).then(setStatus))
  const dismiss = () => run('', () => api('/update/dismiss', { method: 'POST' }).then(setStatus))

  /** Install, rollback or restart: the server goes away and comes back. @param {string} text @param {() => Promise<any>} req */
  async function restarting(text, req) {
    const r = await run(text, req)
    if (r) setRestart({ from: version, at: Date.now() })
  }
  /** @param {boolean} allowOlder */
  const install = (allowOlder) =>
    restarting('Backup, Installieren …', () => api('/update/install', { body: { allow_older: allowOlder } }))

  if (!status) {
    return (
      <Panel version={version}>
        {error ? <p className="error">{error}</p> : <span className="muted small">Lade …</span>}
      </Panel>
    )
  }
  const staged = status.staged
  const locked = running || !!busy || !!restart
  const lockedTitle = running ? 'Erst die Session stoppen' : ''

  return (
    <Panel version={version}>
      {restart && (
        <div className="notice">
          <ArrowsClockwiseIcon size={16} /> Der Laptimer startet neu … die Seite lädt sich danach selbst neu.
        </div>
      )}
      {status.failed && (
        <div className="notice error-notice">
          <span>
            Das Update auf {status.failed.to} ist nicht hochgekommen; der Laptimer ist automatisch zurück auf {status.failed.from}.
          </span>
          <button className="btn btn-ghost" onClick={dismiss}>
            OK
          </button>
        </div>
      )}

      {!status.installed ? (
        <span className="muted small">
          Updates, Neustart und Herunterfahren gibt es nur bei einer installierten Version (<code>install.sh</code> auf dem Pi), nicht bei{' '}
          <code>cargo run</code>.
        </span>
      ) : (
        <>
          {staged ? (
            <div className="staged">
              <span>
                <b>Version {staged.version}</b> ist bereit <span className="muted small">({staged.source}, Signatur geprüft)</span>
              </span>
              {staged.order !== 'newer' && (
                <span className="error small">
                  {staged.order === 'older'
                    ? `Älter als die laufende Version ${version}.`
                    : staged.order === 'same'
                      ? 'Das ist die laufende Version.'
                      : `Nicht sicher neuer als die laufende Version ${version}.`}{' '}
                  Ältere Versionen kennen neuere Daten eventuell nicht; das Backup vorher bleibt.
                </span>
              )}
              <span className="muted small">Vorher wird ein Backup gemacht; der Laptimer startet danach neu (einige Sekunden ohne Verbindung).</span>
              <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                <button
                  className={staged.order === 'newer' ? 'btn btn-primary' : 'btn btn-danger'}
                  disabled={locked}
                  title={lockedTitle}
                  onClick={() => install(staged.order !== 'newer')}
                >
                  {staged.order === 'newer' ? 'Installieren' : 'Trotzdem installieren'}
                </button>
                <button className="btn btn-ghost" disabled={!!busy || !!restart} onClick={discard}>
                  Verwerfen
                </button>
              </div>
            </div>
          ) : (
            <span className="muted small">
              Update: Paket und Signatur aus dem Release laden (<code>laptimer-…-aarch64.tar.gz</code> und <code>….minisig</code>) und hier beide zusammen
              wählen{status.online ? ', oder direkt bei Forgejo suchen' : ''}. Nur von uns signierte Pakete werden angenommen.
            </span>
          )}
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
            <button className="btn btn-secondary" disabled={!!busy || !!restart} onClick={() => fileInput.current?.click()}>
              <UploadSimpleIcon size={16} /> Paket hochladen
            </button>
            <input
              ref={fileInput}
              type="file"
              multiple
              accept=".gz,.minisig,application/gzip"
              style={{ display: 'none' }}
              onChange={(e) => upload(e.target.files)}
            />
            {status.online && (
              <button className="btn btn-secondary" disabled={!!busy || !!restart} onClick={search}>
                <CloudArrowDownIcon size={16} /> Nach Updates suchen
              </button>
            )}
          </div>
          {releases && !releases.length && <span className="muted small">Keine Releases gefunden.</span>}
          {releases && releases.length > 0 && (
            <div>
              {releases.map((r) => (
                <div key={r.tag} className="list-row">
                  <span style={{ display: 'flex', flexDirection: 'column', flex: 1, padding: '6px 0' }}>
                    <span>
                      {r.version} {r.prerelease && <span className="muted small">· Vorabversion</span>}
                    </span>
                    <span className="muted small">
                      {r.published_at ? new Date(r.published_at).toLocaleDateString('de-DE') : ''} · {ORDER[r.order]}
                      {!r.available && ' · ohne Paket für diesen Rechner'}
                    </span>
                  </span>
                  {r.available && r.order !== 'same' && (
                    <button className="btn btn-ghost" disabled={!!busy || !!restart} onClick={() => download(r.tag)}>
                      Holen
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}
        </>
      )}
      {busy && <span className="small">{busy}</span>}
      {error && <p className="error">{error}</p>}

      {status.installed && (
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', borderTop: '1px solid var(--divider)', paddingTop: 10 }}>
          {status.previous && (
            <button
              className="btn btn-ghost"
              disabled={locked}
              title={lockedTitle}
              onClick={() =>
                setConfirm({
                  title: `Zurück auf ${status.previous}?`,
                  text: `Der Laptimer startet mit der vorigen Version ${status.previous} neu; ${version} bleibt und lässt sich genauso wieder zurückholen.`,
                  label: 'Zurück',
                  run: () => restarting('Zurück …', () => api('/update/rollback', { method: 'POST' })),
                })
              }
            >
              <ArrowCounterClockwiseIcon size={16} /> Zurück auf {status.previous}
            </button>
          )}
          {status.managed && (
            <button
              className="btn btn-ghost"
              disabled={locked}
              title={lockedTitle}
              onClick={() =>
                setConfirm({
                  title: 'Laptimer neu starten?',
                  text: 'Der Server startet neu (einige Sekunden); die Boards werden dabei neu eingestellt.',
                  label: 'Neu starten',
                  run: () => restarting('Neustart …', () => api('/system/restart', { method: 'POST' })),
                })
              }
            >
              <ArrowsClockwiseIcon size={16} /> Laptimer neu starten
            </button>
          )}
          {status.power && (
            <>
              <button
                className="btn btn-ghost"
                disabled={locked}
                title={lockedTitle}
                onClick={() =>
                  setConfirm({
                    title: 'Pi neu starten?',
                    text: 'Der Raspberry Pi startet neu; nach etwa einer Minute ist der Laptimer wieder da.',
                    label: 'Pi neu starten',
                    run: () => restarting('Pi startet neu …', () => api('/system/reboot', { method: 'POST' })),
                  })
                }
              >
                <ArrowsClockwiseIcon size={16} /> Pi neu starten
              </button>
              <button
                className="btn btn-ghost"
                disabled={locked}
                title={lockedTitle}
                onClick={() =>
                  setConfirm({
                    title: 'Pi herunterfahren?',
                    text: 'Der Raspberry Pi fährt herunter. Wieder an geht er nur, indem man den Strom kurz trennt.',
                    label: 'Herunterfahren',
                    run: () =>
                      run('', () => api('/system/poweroff', { method: 'POST' })).then(
                        (r) => r && setError('Der Pi fährt herunter. Strom erst trennen, wenn die grüne LED aus ist.'),
                      ),
                  })
                }
              >
                <PowerIcon size={16} /> Herunterfahren
              </button>
            </>
          )}
          {!status.managed && (
            <span className="muted small">
              Der Laptimer läuft nicht als Dienst (systemd): nach einem Update startet er nicht von selbst neu.
            </span>
          )}
        </div>
      )}

      {confirm && (
        <Dialog title={confirm.title} onClose={() => setConfirm(null)}>
          <p className="muted" style={{ margin: 0 }}>
            {confirm.text}
          </p>
          <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
            <button className="btn btn-secondary" onClick={() => setConfirm(null)}>
              Abbrechen
            </button>
            <button
              className="btn btn-danger"
              onClick={() => {
                const c = confirm
                setConfirm(null)
                c.run()
              }}
            >
              {confirm.label}
            </button>
          </div>
        </Dialog>
      )}
    </Panel>
  )
}

/** @param {{version: string, children: React.ReactNode}} props */
function Panel({ version, children }) {
  return (
    <div className="panel">
      <div className="panel-head">
        <CpuIcon size={18} /> System
        <span className="end muted small">Version {version}</span>
      </div>
      <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        {children}
      </div>
    </div>
  )
}
