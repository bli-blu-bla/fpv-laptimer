// @ts-check
// Talking to the server: REST calls for actions, one WebSocket for
// everything that changes.

import { useEffect, useRef, useState } from 'react'
import { announce } from './announcer.js'
import { loginNeeded } from './auth.js'

/**
 * @param {string} path  e.g. "/session/start"
 * @param {{method?: string, body?: unknown, form?: FormData}} [opts]  `form`: a file upload instead of JSON
 * @returns {Promise<any>}
 */
export async function api(path, opts = {}) {
  const json = opts.body !== undefined
  const res = await fetch('/api' + path, {
    method: opts.method ?? (json || opts.form ? 'POST' : 'GET'),
    headers: json ? { 'content-type': 'application/json' } : {},
    body: opts.form ?? (json ? JSON.stringify(opts.body) : undefined),
  })
  const data = await res.json().catch(() => ({}))
  // Changing needs the login (auth.js): ask for it.
  if (res.status === 401 && data.login) loginNeeded()
  if (!res.ok) throw new Error(data.error ?? res.statusText)
  return data
}

/** The server sends `live` 10 times a second: this long without any message, the connection is dead. */
const SILENT_MS = 3000
/** A message this late (server time) comes from a backlog, e.g. after the screen lock: not announced. */
const LATE_US = 2_000_000
/** Clock samples kept for the offset (10 s of `live`). By count, not by time: after a
 * sleep the samples from before still hold, and a backlog shows up as late against them. */
const CLOCK_SAMPLES = 100

/**
 * The server's state, kept current over the WebSocket (reconnects by itself).
 * `clockOffset`: server clock − browser clock, µs, for running timers.
 * `changed`: counts `session_changed` messages and reconnects (pages reload on a change).
 * `flash`: the latest `flash` message (a board being flashed by the server).
 * `announce` messages go to `announcer.js` (plays them if this browser is a speaker).
 *
 * After a screen lock or sleep the old connection often stays "open" without
 * anything arriving, and the page would show a race that is long over. So: no
 * message for `SILENT_MS`, or the page becomes visible again with nothing
 * fresh, means a new connection at once; until its first `state` the page
 * counts as not connected.
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
    /** Browser time of the last message, ms. */
    let lastMessage = 0
    /** Whether a connection has had its `state` before (a later one reloads the pages). */
    let hadState = false
    /** Offset samples (server − browser, µs). Each one is at most the true offset
     * (the message took a while), so the largest is the best. */
    /** @type {number[]} */
    let samples = []
    /** How late the last timed message arrived, µs. */
    let lateness = 0

    /** @param {number} serverUs */
    function clockSample(serverUs) {
      const browserUs = Date.now() * 1000
      samples.push(serverUs - browserUs)
      if (samples.length > CLOCK_SAMPLES) samples.shift()
      clockOffset.current = Math.max(...samples)
      lateness = browserUs + clockOffset.current - serverUs
    }

    function connect() {
      clearTimeout(retry)
      const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
      const socket = new WebSocket(`${proto}//${location.host}/ws`)
      ws = socket
      lastMessage = Date.now()
      // A new connection starts the samples afresh (in case a clock was set meanwhile).
      samples = []
      // Handlers check `ws === socket`: a replaced connection may still deliver its backlog or close late.
      socket.onclose = () => {
        if (ws !== socket) return
        setConnected(false)
        if (!closed) retry = setTimeout(connect, 1000)
      }
      socket.onmessage = (ev) => {
        if (ws !== socket) return
        lastMessage = Date.now()
        const msg = JSON.parse(ev.data)
        if (msg.now_us) clockSample(msg.now_us)
        if (msg.type === 'state') {
          setState(msg)
          setConnected(true)
          // Missed `session_changed` while away: let the pages reload.
          if (hadState) setChanged((n) => n + 1)
          hadState = true
        } else if (msg.type === 'live') setLive(msg)
        else if (msg.type === 'session_changed') setChanged((n) => n + 1)
        else if (msg.type === 'flash') setFlash(msg)
        else if (msg.type === 'announce' && lateness < LATE_US) announce(msg.items)
      }
    }

    /** Drops the current connection without waiting for its close, and connects anew. */
    function reconnect() {
      if (closed) return
      const old = ws
      ws = null
      old?.close()
      setConnected(false)
      connect()
    }

    // Silent too long, or stuck connecting (also catches a timer that slept: the gap is then huge).
    const watchdog = setInterval(() => {
      if (ws && ws.readyState <= WebSocket.OPEN && Date.now() - lastMessage > SILENT_MS) reconnect()
    }, 1000)
    // Back from the screen lock: check at once instead of up to `SILENT_MS` later.
    function wake() {
      if (document.visibilityState !== 'visible') return
      // `lastMessage` is also set when connecting: a fresh attempt may go on.
      if (Date.now() - lastMessage > 1000 || !ws || ws.readyState > WebSocket.OPEN) reconnect()
    }
    document.addEventListener('visibilitychange', wake)
    window.addEventListener('pageshow', wake)
    window.addEventListener('online', wake)

    connect()
    return () => {
      closed = true
      clearTimeout(retry)
      clearInterval(watchdog)
      document.removeEventListener('visibilitychange', wake)
      window.removeEventListener('pageshow', wake)
      window.removeEventListener('online', wake)
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
