// @ts-check
// Talking to the server: REST calls for actions, one WebSocket for
// everything that changes.

import { useEffect, useRef, useState } from 'react'
import { announce } from './announcer.js'

/**
 * @param {string} path  e.g. "/session/start"
 * @param {{method?: string, body?: unknown}} [opts]
 * @returns {Promise<any>}
 */
export async function api(path, opts = {}) {
  const res = await fetch('/api' + path, {
    method: opts.method ?? (opts.body === undefined ? 'GET' : 'POST'),
    headers: opts.body === undefined ? {} : { 'content-type': 'application/json' },
    body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
  })
  const data = await res.json().catch(() => ({}))
  if (!res.ok) throw new Error(data.error ?? res.statusText)
  return data
}

/**
 * The server's state, kept current over the WebSocket (reconnects by itself).
 * `clockOffset`: server clock − browser clock, µs, for running timers.
 * `changed`: counts `session_changed` messages (pages reload on a change).
 * `flash`: the latest `flash` message (a board being flashed by the server).
 * `announce` messages go to `announcer.js` (plays them if this browser is a speaker).
 */
export function useServer() {
  const [state, setState] = useState(/** @type {import('./types').State | null} */ (null))
  const [live, setLive] = useState(/** @type {import('./types').Live | null} */ (null))
  const [connected, setConnected] = useState(false)
  const [changed, setChanged] = useState(0)
  const [flash, setFlash] = useState(/** @type {import('./types').FlashStatus | null} */ (null))
  const clockOffset = useRef(0)

  useEffect(() => {
    /** @type {WebSocket | null} */
    let ws = null
    let closed = false
    /** @type {ReturnType<typeof setTimeout> | undefined} */
    let retry

    function connect() {
      const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
      ws = new WebSocket(`${proto}//${location.host}/ws`)
      ws.onopen = () => setConnected(true)
      ws.onclose = () => {
        setConnected(false)
        if (!closed) retry = setTimeout(connect, 1000)
      }
      ws.onmessage = (ev) => {
        const msg = JSON.parse(ev.data)
        if (msg.now_us) clockOffset.current = msg.now_us - Date.now() * 1000
        if (msg.type === 'state') setState(msg)
        else if (msg.type === 'live') setLive(msg)
        else if (msg.type === 'session_changed') setChanged((n) => n + 1)
        else if (msg.type === 'flash') setFlash(msg)
        else if (msg.type === 'announce') announce(msg.items)
      }
    }
    connect()
    return () => {
      closed = true
      clearTimeout(retry)
      ws?.close()
    }
  }, [])

  return { state, live, connected, changed, flash, clockOffset }
}

/** Server time now, µs. @param {{current: number}} clockOffset */
export function serverNow(clockOffset) {
  return Date.now() * 1000 + clockOffset.current
}

/** Saves the settings with `patch` applied (the server sends them back
 * to every page through the WebSocket).
 * @param {import('./types').Settings} current @param {Partial<import('./types').Settings>} patch */
export function saveSettings(current, patch) {
  return api('/settings', { method: 'PUT', body: { ...current, ...patch } })
}

/** The server time, updated 10 times a second while `running`.
 * @param {{current: number}} clockOffset @param {boolean} running */
export function useNow(clockOffset, running) {
  const [now, setNow] = useState(() => serverNow(clockOffset))
  useEffect(() => {
    if (!running) return
    const id = setInterval(() => setNow(serverNow(clockOffset)), 100)
    return () => clearInterval(id)
  }, [clockOffset, running])
  return now
}
