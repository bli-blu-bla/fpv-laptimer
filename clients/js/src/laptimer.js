// @ts-check
// Client for the lap timer's public API (/api/v1, docs/api/).
// No dependencies: global fetch and WebSocket (Node >= 22, browsers).

/** @import { Channel, ChannelChange, EventMap, EventMessage, Hello, Pilot, Session, StartOptions, State, Stats } from './types.js' */

/**
 * An error from the server (its message is German, meant to be shown as is)
 * or from the connection.
 */
export class LaptimerError extends Error {
  /**
   * @param {string} message
   * @param {object} [info]
   * @param {number | null} [info.status]   HTTP status (400 bad arguments, 401 key, 404 unknown command), the same over the WebSocket; null without an answer
   * @param {string} [info.command]
   * @param {unknown} [info.cause]
   */
  constructor(message, { status = null, command, cause } = {}) {
    super(message, { cause })
    this.name = 'LaptimerError'
    /** @type {number | null} */
    this.status = status
    /** @type {string | undefined} */
    this.command = command
  }
}

/**
 * What `on` can listen to: the server's events, `'*'` (every event) and
 * three of the client's own: `hello` (connected, with the state),
 * `lagged` (events were lost: fetch the state again) and `disconnected`.
 * @typedef {EventMap & {
 *   '*': any,
 *   hello: Hello,
 *   lagged: {missed: number},
 *   disconnected: {code: number, reason: string},
 * }} ClientEvents
 */

/**
 * @template {string} K
 * @typedef {(data: K extends keyof ClientEvents ? ClientEvents[K] : any, msg: EventMessage<any> | any) => void} Listener
 */

/**
 * @typedef {object} LaptimerOptions
 * @property {string} url                 e.g. "http://pi:5800"
 * @property {string} key                 API key "lt_…"
 * @property {'auto' | 'http'} [transport]  auto (default): commands over the WebSocket while it is connected, else HTTP
 * @property {number} [timeout_ms]        per command, default 15000
 * @property {number} [max_backoff_ms]    longest wait between reconnects, default 10000
 */

export class Laptimer {
  /** @type {WebSocket | null} */
  #ws = null
  /** true between connect() and close(): reconnect when the socket drops. */
  #wanted = false
  /** The socket got its `hello` (commands may go over it). */
  #ready = false
  #attempt = 0
  /** @type {ReturnType<typeof setTimeout> | null} */
  #retryTimer = null
  /** @type {Promise<Hello> | null} */
  #connecting = null
  #nextId = 1
  /** Commands sent over the WebSocket, waiting for their reply (by id). @type {Map<number, {resolve: (v: any) => void, reject: (e: Error) => void, timer: ReturnType<typeof setTimeout>, cmd: string}>} */
  #pending = new Map()
  /** @type {Map<string, Set<Function>>} */
  #listeners = new Map()

  /** @param {LaptimerOptions} options */
  constructor({ url, key, transport = 'auto', timeout_ms = 15000, max_backoff_ms = 10000 }) {
    if (!url) throw new TypeError("Laptimer: 'url' fehlt, z. B. http://pi:5800")
    if (!key) throw new TypeError("Laptimer: 'key' fehlt (API-Schlüssel lt_…)")
    this.url = url.replace(/\/+$/, '')
    this.key = key
    this.transport = transport
    this.timeout_ms = timeout_ms
    this.max_backoff_ms = max_backoff_ms

    // The commands by group, as in the API (channels.set → client.channels.set).
    this.channels = {
      /** @returns {Promise<Channel[]>} */
      list: () => this.command('channels.list'),
      /**
       * One channel; only the given fields change. A pilot or frequency another
       * channel has is swapped with it.
       * @param {number} index  0 = first channel
       * @param {ChannelChange} change
       * @returns {Promise<Channel>}
       */
      set: (index, change) => this.command('channels.set', { ...change, index }),
      /**
       * All channels at once, in order: callsign, pilot id or null per channel.
       * @param {(string | number | null)[]} pilots
       * @param {{create?: boolean}} [options]  create: make unknown callsigns pilots
       * @returns {Promise<Channel[]>}
       */
      assign: (pilots, { create } = {}) => this.command('channels.assign', { pilots, create }),
    }

    this.pilots = {
      /** @returns {Promise<Pilot[]>} */
      list: () => this.command('pilots.list'),
      /** @param {number} id @returns {Promise<Pilot>} */
      get: (id) => this.command('pilots.get', { id }),
      /** @param {string} name callsign @param {string} [full_name] @returns {Promise<Pilot>} */
      create: (name, full_name) => this.command('pilots.create', { name, full_name }),
      /** @param {number} id @param {{name?: string, full_name?: string}} change @returns {Promise<Pilot>} */
      update: (id, change) => this.command('pilots.update', { ...change, id }),
      /** @param {number} id @returns {Promise<{}>} */
      delete: (id) => this.command('pilots.delete', { id }),
    }

    this.session = {
      /**
       * Starts a training (default) or a race. Refused while a session runs or
       * a race waits for confirmation.
       * @param {StartOptions} [options]
       * @returns {Promise<Session>}
       */
      start: (options = {}) => this.command('session.start', options),
      /** @returns {Promise<{session_id: number}>} */
      stop: () => this.command('session.stop'),
      /** Stops and deletes the running session. @returns {Promise<{session_id: number}>} */
      cancel: () => this.command('session.cancel'),
      /** @param {number} [id] default: the race waiting for confirmation @returns {Promise<{session_id: number}>} */
      confirm: (id) => this.command('session.confirm', { id }),
      /** Deletes an ended session. @param {number} [id] default: the race waiting for confirmation @returns {Promise<{session_id: number}>} */
      discard: (id) => this.command('session.discard', { id }),
    }

    this.sessions = {
      /** @param {{limit?: number}} [options] default 20 @returns {Promise<Session[]>} */
      list: ({ limit } = {}) => this.command('sessions.list', { limit }),
      /** With every pass (`passes`). @param {number} id @returns {Promise<Session>} */
      get: (id) => this.command('sessions.get', { id }),
    }

    this.passes = {
      /**
       * A pass by hand: counts like a detected one (announcement, `pilot_pass`).
       * @param {number} channel  index
       * @param {{time_us?: number, session_id?: number}} [options]  default: now, the running session
       * @returns {Promise<{pass_id: number, session_id: number, time_us: number}>}
       */
      add: (channel, { time_us, session_id } = {}) => this.command('passes.add', { channel, time_us, session_id }),
      /** Detected passes are only marked (restorable). @param {number} id @returns {Promise<{session_id: number}>} */
      delete: (id) => this.command('passes.delete', { id }),
      /** @param {number} id @returns {Promise<{session_id: number}>} */
      restore: (id) => this.command('passes.restore', { id }),
    }
  }

  /** @returns {Promise<{pong: true, now_us: number}>} */
  ping() {
    return this.command('ping')
  }

  /** @returns {Promise<State>} */
  state() {
    return this.command('state')
  }

  /** @returns {Promise<Stats>} */
  stats() {
    return this.command('stats')
  }

  /** True while the WebSocket is connected (and commands go over it with transport "auto"). */
  get connected() {
    return this.#ready
  }

  // --- commands ---

  /**
   * Any command by name, e.g. `command('session.start', {race: true})`.
   * Arguments that are `undefined` are left out.
   * @param {string} name
   * @param {Record<string, unknown>} [args]
   * @returns {Promise<any>}
   */
  command(name, args = {}) {
    // JSON.stringify drops undefined fields; the server treats missing as default.
    const clean = JSON.parse(JSON.stringify(args))
    if (this.transport === 'auto' && this.#ready) return this.#wsCommand(name, clean)
    return this.#httpCommand(name, clean)
  }

  /** `POST /api/v1/cmd/<name>`: one route for every command (same as the REST routes). @param {string} name @param {object} args */
  async #httpCommand(name, args) {
    let res
    try {
      res = await fetch(`${this.url}/api/v1/cmd/${encodeURIComponent(name)}`, {
        method: 'POST',
        headers: { authorization: `Bearer ${this.key}`, 'content-type': 'application/json' },
        body: JSON.stringify(args),
        signal: AbortSignal.timeout(this.timeout_ms),
      })
    } catch (e) {
      throw new LaptimerError(`Laptimer nicht erreichbar (${this.url}): ${e instanceof Error ? e.message : e}`, { command: name, cause: e })
    }
    const text = await res.text()
    /** @type {any} */
    let body = null
    try {
      body = text ? JSON.parse(text) : null
    } catch {
      // not JSON (e.g. a proxy's error page): the text is the message
    }
    if (!res.ok) {
      const message = body?.error ?? (text || `HTTP ${res.status}`)
      throw new LaptimerError(message, { status: res.status, command: name })
    }
    return body
  }

  /** @param {string} name @param {object} args @returns {Promise<any>} */
  #wsCommand(name, args) {
    const id = this.#nextId++
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id)
        reject(new LaptimerError(`Keine Antwort auf '${name}' nach ${this.timeout_ms} ms.`, { command: name }))
      }, this.timeout_ms)
      this.#pending.set(id, { resolve, reject, timer, cmd: name })
      this.#ws?.send(JSON.stringify({ id, cmd: name, args }))
    })
  }

  // --- events ---

  /**
   * Listens to an event (`pilot_pass`, …), to `'*'` (every server event) or
   * to `hello`, `lagged`, `disconnected`. The listener gets `data` and the
   * whole message. Returns a function that removes the listener.
   * @template {string} K
   * @param {K} name
   * @param {Listener<K>} fn
   * @returns {() => void}
   */
  on(name, fn) {
    let set = this.#listeners.get(name)
    if (!set) this.#listeners.set(name, (set = new Set()))
    set.add(fn)
    return () => this.off(name, fn)
  }

  /** @template {string} K @param {K} name @param {Listener<K>} fn */
  off(name, fn) {
    this.#listeners.get(name)?.delete(fn)
  }

  /**
   * Waits for the next event `name` for which `filter` is true (or for any,
   * without a filter); resolves with its data. Rejects after `timeout_ms`.
   * Needs a connected WebSocket (`connect()`).
   * @template {string} K
   * @param {K} name
   * @param {{filter?: (data: K extends keyof ClientEvents ? ClientEvents[K] : any) => boolean, timeout_ms?: number}} [options]
   * @returns {Promise<K extends keyof ClientEvents ? ClientEvents[K] : any>}
   */
  waitFor(name, { filter, timeout_ms = 30000 } = {}) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        off()
        reject(new LaptimerError(`Event '${name}' kam nicht innerhalb von ${timeout_ms} ms.`))
      }, timeout_ms)
      const off = this.on(name, (data) => {
        if (filter && !filter(data)) return
        clearTimeout(timer)
        off()
        resolve(data)
      })
    })
  }

  /** @param {string} name @param {unknown} data @param {unknown} msg */
  #emit(name, data, msg) {
    for (const fn of [...(this.#listeners.get(name) ?? [])]) {
      try {
        fn(data, msg)
      } catch (e) {
        // One broken listener must not stop the others or the connection.
        console.error(`laptimer: Fehler im Listener für '${name}':`, e)
      }
    }
  }

  // --- WebSocket ---

  /**
   * Opens the WebSocket; resolves with the `hello` (contains `state`).
   * Once connected, a dropped connection is opened again by itself (waiting
   * 0.5 s, 1 s, 2 s, … up to `max_backoff_ms`) until `close()`. If the very
   * first attempt fails (wrong address or key), the promise rejects and
   * nothing is retried.
   * @returns {Promise<Hello>}
   */
  connect() {
    if (this.#connecting) return this.#connecting
    this.#wanted = true
    this.#connecting = this.#open(true)
    return this.#connecting
  }

  /** Closes the WebSocket for good (no reconnect). Commands go over HTTP again. */
  close() {
    this.#wanted = false
    this.#connecting = null
    if (this.#retryTimer) clearTimeout(this.#retryTimer)
    this.#retryTimer = null
    this.#ws?.close()
  }

  /** @param {boolean} first  the first attempt of connect() @returns {Promise<Hello>} */
  #open(first) {
    return new Promise((resolve, reject) => {
      // Browsers can't set headers on a WebSocket: the key goes into the query.
      const wsUrl = this.url.replace(/^http/, 'ws') + `/api/v1/ws?key=${encodeURIComponent(this.key)}`
      const ws = new WebSocket(wsUrl)
      this.#ws = ws
      let gotHello = false

      ws.addEventListener('message', (ev) => {
        /** @type {any} */
        let msg
        try {
          msg = JSON.parse(String(ev.data))
        } catch {
          return
        }
        if (msg.type === 'hello') {
          gotHello = true
          this.#ready = true
          this.#attempt = 0
          resolve(msg)
          this.#emit('hello', msg, msg)
        } else if (msg.type === 'event') {
          this.#emit(msg.event, msg.data, msg)
          this.#emit('*', msg.data, msg)
        } else if (msg.type === 'reply') {
          const p = this.#pending.get(msg.id)
          if (!p) return
          this.#pending.delete(msg.id)
          clearTimeout(p.timer)
          if (msg.ok) p.resolve(msg.data)
          else p.reject(new LaptimerError(msg.error, { status: msg.status ?? null, command: p.cmd }))
        } else if (msg.type === 'lagged') {
          this.#emit('lagged', { missed: msg.missed }, msg)
        }
      })

      ws.addEventListener('close', (ev) => {
        if (this.#ws !== ws) return // an old socket
        this.#ws = null
        this.#ready = false
        for (const [id, p] of this.#pending) {
          clearTimeout(p.timer)
          p.reject(new LaptimerError(`Verbindung getrennt, bevor '${p.cmd}' beantwortet wurde.`, { command: p.cmd }))
          this.#pending.delete(id)
        }
        if (gotHello) this.#emit('disconnected', { code: ev.code, reason: ev.reason }, null)
        if (!gotHello && first) {
          // Never connected: no retry. Ask over HTTP why (401 says "key"), so
          // the error is the server's own message.
          this.#wanted = false
          this.#connecting = null
          this.ping().then(
            () => reject(new LaptimerError(`WebSocket ${wsUrl.replace(/key=[^&]*/, 'key=…')} ließ sich nicht öffnen.`)),
            (e) => reject(e),
          )
          return
        }
        if (this.#wanted) this.#scheduleReconnect()
      })
      // 'error' is always followed by 'close', which handles it.
      ws.addEventListener('error', () => {})
    })
  }

  #scheduleReconnect() {
    const delay = Math.min(500 * 2 ** this.#attempt, this.max_backoff_ms)
    this.#attempt++
    this.#retryTimer = setTimeout(() => {
      this.#retryTimer = null
      if (!this.#wanted) return
      // Failed attempts end in 'close' again and come back here.
      this.#open(false).catch(() => {})
    }, delay)
  }
}
