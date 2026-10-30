// @ts-check
// The JSON the server sends (docs/api/datenformate.md and events.md).
// Only typedefs: this file exports nothing at runtime.

/**
 * A channel (a seat on the page: slot 1 = index 0).
 * @typedef {object} Channel
 * @property {number} index         0, 1, …
 * @property {string} name          "R1" … "R8" or the MHz as text
 * @property {number} mhz
 * @property {boolean} enabled      false: does not fly along
 * @property {number | null} pilot_id
 * @property {string | null} pilot  callsign of the assigned pilot
 * @property {string} label         free text instead of a pilot (wins over `pilot`)
 * @property {string} shown         what the page shows
 */

/**
 * @typedef {object} Pilot
 * @property {number} id
 * @property {string} name        callsign (unique)
 * @property {string} full_name   may be empty
 */

/**
 * One seat's result in a session (`results`, in seat order).
 * @typedef {object} SeatResult
 * @property {number} channel
 * @property {string} channel_name
 * @property {number} mhz
 * @property {number | null} pilot_id
 * @property {string} pilot              shown name: callsign, free text or channel name
 * @property {number | null} position    null without a lap
 * @property {number} laps
 * @property {number[]} lap_times_us
 * @property {number | null} last_lap_us
 * @property {number | null} best_lap_us
 * @property {number | null} best_consecutive_us
 * @property {number | null} total_us
 * @property {number | null} holeshot_us
 * @property {boolean} finished
 */

/**
 * A pass through the gate (only in `sessions.get`).
 * @typedef {object} Pass
 * @property {number} id
 * @property {number} channel
 * @property {number} time_us
 * @property {number} peak_db
 * @property {boolean} manual    added by hand
 * @property {boolean} deleted
 * @property {number} gate        0 = finish, 1 = arming gate
 * @property {'no_arming' | null} [rejected]   deleted by the server, and why
 * @property {number | null} [detected_us]   where the detector put it (null: added by hand; differs from time_us: moved by hand)
 */

/** @typedef {'holeshot' | 'from_start' | 'flying'} StartMode */

/**
 * @typedef {object} Session
 * @property {number} id
 * @property {boolean} race                  false = training
 * @property {'countdown' | 'running' | 'ended'} phase
 * @property {StartMode} start_mode
 * @property {'open' | 'laps' | 'seconds'} end
 * @property {number | null} end_value
 * @property {number} countdown_s
 * @property {number} min_lap_s
 * @property {number} created_us
 * @property {number} start_us
 * @property {number | null} ended_us
 * @property {number | null} confirmed_us
 * @property {boolean} needs_confirmation
 * @property {SeatResult[]} results
 * @property {Pass[]} [passes]               only from `sessions.get`
 */

/**
 * @typedef {object} Board
 * @property {string} id
 * @property {string} port
 * @property {string} role       finish, arming or off
 * @property {boolean} ok
 * @property {string | null} error
 * @property {number} levels_per_s
 */

/**
 * @typedef {object} State
 * @property {number} now_us
 * @property {Session | null} session        the running session
 * @property {Session | null} unconfirmed    the ended race waiting for confirmation
 * @property {Channel[]} channels
 * @property {Pilot[]} pilots
 * @property {Board[]} boards
 */

/**
 * @typedef {object} PilotStats
 * @property {number | null} pilot_id
 * @property {string} label
 * @property {number | null} best_lap_us
 * @property {number | null} best_consecutive_us
 * @property {number} sessions
 * @property {number} races
 * @property {number} wins
 */

/**
 * @typedef {object} Stats
 * @property {number} today_races
 * @property {PilotStats[]} all     best lap first
 * @property {PilotStats[]} today
 */

/**
 * Arguments of `session.start`.
 * @typedef {object} StartOptions
 * @property {boolean} [race]          true: race, false: training (default)
 * @property {number} [laps]           race: end after this many laps (1–100)
 * @property {number} [seconds]        race: end after this many seconds (10–3600), instead of laps
 * @property {number} [countdown_s]    race: 0–60
 * @property {StartMode} [start_mode]  race
 */

/**
 * Arguments of `channels.set` (only given fields change).
 * @typedef {object} ChannelChange
 * @property {string | null} [pilot]      callsign, null = nobody
 * @property {number | null} [pilot_id]   instead of `pilot`
 * @property {boolean} [create]           make an unknown callsign a pilot
 * @property {string} [label]             free text instead of a pilot ("" = none)
 * @property {boolean} [enabled]
 * @property {string | number} [frequency]  "R1" … "R8" or MHz (5300–6000); not during a session
 */

// --- events ---

/**
 * Fields every event about a pilot has ("Sitz" in docs/api/events.md).
 * @typedef {object} Seat
 * @property {number} channel
 * @property {string} channel_name
 * @property {number} mhz
 * @property {number | null} pilot_id
 * @property {string} pilot
 */

/**
 * @typedef {object} SessionStartedData
 * @property {number} session_id
 * @property {boolean} race
 * @property {StartMode} start_mode
 * @property {'open' | 'laps' | 'seconds'} end
 * @property {number | null} end_value
 * @property {number} countdown_s
 * @property {number} created_us
 * @property {number} start_us
 * @property {number} pilot_count
 * @property {Seat[]} seats
 */

/**
 * @typedef {object} CountdownData
 * @property {number} session_id
 * @property {boolean} race
 * @property {number} seconds_left
 */

/**
 * @typedef {object} StartSignalData
 * @property {number} session_id
 * @property {boolean} race
 * @property {number} start_us
 */

/**
 * @typedef {Seat & {
 *   session_id: number, race: boolean, pass_id: number, time_us: number, manual: boolean,
 *   kind: 'lap' | 'holeshot' | 'start' | 'ignored',
 *   lap: number | null, lap_time_us: number | null, lap_time_s: number | null, laps: number,
 *   best_lap_us: number | null, best_lap_s: number | null, personal_best: boolean, session_best: boolean,
 *   holeshot_us: number | null, finished: boolean, position: number | null
 * }} PilotPassData
 */

/**
 * @typedef {Seat & {
 *   session_id: number, laps: number, total_us: number | null, total_s: number | null,
 *   best_lap_us: number | null, best_lap_s: number | null, position: number | null
 * }} PilotFinishedData
 */

/**
 * @typedef {object} SessionStoppedData
 * @property {number} session_id
 * @property {boolean} race
 * @property {'stopped' | 'finished' | 'cancelled'} reason
 * @property {number} ended_us
 * @property {boolean} needs_confirmation
 * @property {string | null} winner      race only
 * @property {SeatResult[]} results
 */

/**
 * @typedef {object} RaceConfirmedData
 * @property {number} session_id
 * @property {boolean} race
 * @property {number} confirmed_us
 * @property {string | null} winner
 * @property {SeatResult[]} results
 */

/**
 * @typedef {object} PilotEventData
 * @property {number} pilot_id
 * @property {string} name
 * @property {string} [full_name]   not in pilot_deleted
 */

/**
 * Event name → its `data`.
 * @typedef {object} EventMap
 * @property {SessionStartedData} session_started
 * @property {CountdownData} countdown
 * @property {StartSignalData} start_signal
 * @property {PilotPassData} pilot_pass
 * @property {PilotFinishedData} pilot_finished
 * @property {SessionStoppedData} session_stopped
 * @property {RaceConfirmedData} race_confirmed
 * @property {{session_id: number, race: boolean}} session_deleted
 * @property {{deleted: number}} sessions_reset
 * @property {{session_id: number}} passes_changed
 * @property {{channels: Channel[]}} channels_changed
 * @property {PilotEventData} pilot_added
 * @property {PilotEventData} pilot_updated
 * @property {PilotEventData} pilot_deleted
 */

/**
 * An event as it comes over the WebSocket.
 * @template [D=any]
 * @typedef {object} EventMessage
 * @property {'event'} type
 * @property {string} event
 * @property {number} seq        counts from 1 since the server started; a gap = missed events
 * @property {number} time_us
 * @property {D} data
 */

/**
 * The first message after connecting.
 * @typedef {object} Hello
 * @property {'hello'} type
 * @property {number} api
 * @property {State | null} state
 * @property {string[]} commands
 */

export {}
