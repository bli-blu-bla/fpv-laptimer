// @ts-check
// Boards → Firmware: which firmware each board runs, and two ways to write
// the one this laptimer brings along (firmware.env, tools/fetch_firmware.sh):
// - over the server, for the boards on its USB (the Pi at the gate);
// - over Web Serial, for a board on this computer's USB. The browser allows
//   that only on localhost or HTTPS, so not on the Pi's http://… page.
// The firmware has its own repository and page (FIRMWARE_PAGE): newest
// version, board names, levels and a console, for a board on any computer.

import { useEffect, useState } from 'react'
import { LightningIcon, UploadSimpleIcon } from '@phosphor-icons/react'
import { api } from '../api.js'
import { flashFirmware, loadManifest, requestPort, webSerialAvailable } from '../flasher.js'
import { compareVersions } from '../firmwareVersion.js'

const FIRMWARE_PAGE = 'https://bli-blu-bla.github.io/fpv-laptimer-firmware/'

/** "widscan58|esp32c5|proto|version|…" → version. @param {string | null} info */
const versionOf = (info) => info?.split('|')[3] ?? null
/** The end of the id (the MAC's last three bytes). @param {string} id */
const shortId = (id) => (id.length > 8 ? '…' + id.slice(-8) : id)

/**
 * @param {{state: import('../types').State, flash: import('../types').FlashStatus | null}} props
 */
export default function FirmwarePanel({ state, flash }) {
  const [fw, setFw] = useState(/** @type {import('../types').FirmwareState | null} */ (null))
  const [error, setError] = useState('')
  // The WebSocket's message, or (page opened during a flash) the one from /api/firmware.
  const status = flash ?? fw?.flash ?? null
  const busy = status != null && !status.done
  const sessionRunning = state.active != null
  const boardsKey = state.boards.map((b) => b.port + b.info).join()

  // Read again when a flash ends and when the boards change.
  useEffect(() => {
    api('/firmware')
      .then(setFw)
      .catch((/** @type {Error} */ e) => setError(e.message))
  }, [status?.done, boardsKey])

  /** @param {string} port */
  async function flashOnServer(port) {
    setError('')
    try {
      await api('/firmware/flash', { body: { port } })
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    }
  }

  const offered = fw?.firmware?.version ?? null
  return (
    <div className="panel">
      <div className="panel-head">
        <UploadSimpleIcon size={18} /> Firmware
        <span className="end muted small">{offered ? `Bereitgestellt: ${offered}` : ''}</span>
      </div>
      <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {fw?.error && <span className="error small">{fw.error}</span>}

        {fw && !fw.ports.length && <span className="muted small">Am Server ist kein Board angesteckt.</span>}
        {fw?.ports.map((p) => {
          const version = versionOf(p.info)
          // -1 older, 0 the same, 1 newer (a test build), null not comparable
          const cmp = version && offered ? compareVersions(version, offered) : null
          const mine = status?.port === p.port
          return (
            <div key={p.port} style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                <span style={{ fontWeight: 600 }} className={p.name ? '' : 'tnum'} title={p.id}>
                  {p.name ?? shortId(p.id)}
                </span>
                <span className="muted small" style={{ flex: 1, minWidth: 0 }}>
                  {version ? `Firmware ${version}` : 'Keine Laptimer-Firmware oder nicht erreichbar'}
                </span>
                {cmp === 0 && <span className="tag">aktuell</span>}
                {cmp === -1 && <span className="tag tag-accent">veraltet</span>}
                {cmp === 1 && <span className="tag">neuer</span>}
                {version && offered && cmp == null && version !== offered && <span className="tag">andere Version</span>}
                <button
                  className="btn btn-secondary"
                  disabled={!offered || busy || sessionRunning}
                  onClick={() => flashOnServer(p.port)}
                >
                  <LightningIcon size={16} /> Aufspielen
                </button>
              </div>
              {mine && <Progress status={status} />}
            </div>
          )
        })}
        {status && fw && !fw.ports.some((p) => p.port === status.port) && <Progress status={status} />}
        <span className="muted small">
          {sessionRunning
            ? 'Während einer Session lässt sich nichts flashen.'
            : 'Beim Flashen pausieren alle Boards für etwa 15 Sekunden.'}
        </span>
        {error && <span className="error small">{error}</span>}

        <LocalFlash offered={offered} />
        <span className="muted small">
          Neueste Firmware, Board-Namen und Pegel auch an jedem anderen Rechner:{' '}
          <a href={FIRMWARE_PAGE} target="_blank" rel="noreferrer">
            Firmware-Seite
          </a>{' '}
          (Chrome oder Edge).
        </span>
      </div>
    </div>
  )
}

/** @param {{status: {stage: string, progress: number | null, done: boolean, error: string | null}}} props */
function Progress({ status }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
      <span className="small" style={status.done && !status.error ? { color: 'var(--accent-300)' } : undefined}>
        {status.stage}
        {status.progress != null && !status.done ? ` ${Math.round(status.progress * 100)} %` : ''}
      </span>
      {status.progress != null && !status.done && (
        <div className="level-bar">
          <div className="fill" style={{ width: status.progress * 100 + '%' }} />
        </div>
      )}
      {status.error && <span className="error small">{status.error}</span>}
    </div>
  )
}

/** A board on this computer's USB, over Web Serial.
 * @param {{offered: string | null}} props */
function LocalFlash({ offered }) {
  const [status, setStatus] = useState(
    /** @type {{stage: string, progress: number | null, done: boolean, error: string | null} | null} */ (null),
  )
  const [log, setLog] = useState(/** @type {string[]} */ ([]))
  const busy = status != null && !status.done

  if (!webSerialAvailable()) {
    return (
      <span className="muted small">
        Ein Board direkt an diesem Gerät flashen: nur in Chrome oder Edge, und nur wenn die Seite über localhost oder HTTPS
        geöffnet ist (z. B. <code>npm run dev</code> auf dem Mac).
      </span>
    )
  }

  async function start() {
    setLog([])
    /** @type {SerialPort} */
    let port
    try {
      port = await requestPort() // must come first: right after the click
    } catch {
      return // chooser closed
    }
    /** @param {Partial<NonNullable<typeof status>>} patch */
    const update = (patch) =>
      setStatus((s) => ({ stage: '', progress: null, done: false, error: null, ...s, ...patch }))
    setStatus({ stage: 'Firmware laden …', progress: null, done: false, error: null })
    try {
      const manifest = await loadManifest()
      await flashFirmware(port, manifest, {
        stage: (stage) => update({ stage }),
        progress: (progress) => update({ progress }),
        log: (line) => setLog((l) => [...l.slice(-30), line]),
      })
      update({ stage: `Fertig: ${manifest.version}`, progress: null, done: true })
    } catch (e) {
      let msg = /** @type {Error} */ (e).message
      if (/open|busy|in use/i.test(msg)) {
        msg += ' – Benutzt ein anderes Programm den Port (der Laptimer-Server, ein Flash-Script)? Ein Board am Server oben über „Aufspielen“ flashen.'
      }
      update({ stage: 'Fehlgeschlagen', progress: null, done: true, error: msg })
    }
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8, borderTop: '1px solid var(--divider)', paddingTop: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <span style={{ flex: 1, minWidth: 0 }} className="small">
          Board an diesem Gerät (USB)
        </span>
        <button className="btn btn-secondary" disabled={!offered || busy} onClick={start}>
          <LightningIcon size={16} /> Board wählen …
        </button>
      </div>
      {status && <Progress status={status} />}
      {status?.error && log.length > 0 && (
        <pre className="muted small" style={{ margin: 0, whiteSpace: 'pre-wrap', maxHeight: 160, overflow: 'auto' }}>
          {log.join('\n')}
        </pre>
      )}
      <span className="muted small">
        Hängt das Board, oder steckt es zum ersten Mal: BOOT halten, RESET drücken, BOOT loslassen, dann noch einmal.
      </span>
    </div>
  )
}
