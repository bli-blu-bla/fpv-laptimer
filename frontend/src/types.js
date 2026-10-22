// What the server sends (server/src/core.rs, db.rs, laps.rs), as JSDoc
// types. They are only for the editor and `npm run check`: JavaScript itself
// ignores them.

/**
 * @typedef {'holeshot' | 'from_start' | 'flying'} StartMode
 * @typedef {{kind: 'open'} | {kind: 'laps', value: number} | {kind: 'seconds', value: number}} EndRule
 *
 * @typedef {object} ChannelSetting
 * @property {string} name
 * @property {number} mhz
 *
 * @typedef {object} SeatSetting
 * @property {number | null} pilot_id
 * @property {string} label  free text; empty: the pilot's callsign or the channel name
 * @property {boolean} enabled  off: nobody flies on this slot
 *
 * @typedef {object} Settings
 * @property {ChannelSetting[]} channels
 * @property {SeatSetting[]} seats
 * @property {number} gain
 * @property {{start_mode: StartMode, end_rule: EndRule, countdown_s: number}} race
 * @property {number} min_lap_s
 * @property {number} consecutive
 * @property {boolean} sound
 *
 * @typedef {object} Pilot
 * @property {number} id
 * @property {string} name       the callsign
 * @property {string} full_name  may be empty
 *
 * @typedef {object} SessionConfig
 * @property {boolean} race
 * @property {StartMode} start_mode
 * @property {EndRule} end_rule
 * @property {number} countdown_s
 * @property {number} min_lap_s
 * @property {number} consecutive
 *
 * @typedef {object} Seat
 * @property {number} channel
 * @property {string} channel_name
 * @property {number} mhz
 * @property {number | null} pilot_id
 * @property {string} label
 *
 * @typedef {object} Session
 * @property {number} id
 * @property {SessionConfig} config
 * @property {number} created_us   wall clock, µs since 1970
 * @property {number} start_us     start signal
 * @property {number | null} ended_us
 * @property {Seat[]} seats
 *
 * @typedef {object} Pass
 * @property {number} id
 * @property {number} channel
 * @property {number} time_us
 * @property {number} peak_db
 * @property {boolean} manual
 * @property {boolean} deleted
 *
 * @typedef {object} Lap
 * @property {number} number
 * @property {number} duration_us
 * @property {number} end_us
 * @property {number} pass_id
 *
 * @typedef {object} PilotResult
 * @property {number} channel
 * @property {number | null} holeshot_us
 * @property {Lap[]} laps
 * @property {number | null} finished_us
 * @property {number | null} total_us
 * @property {number | null} best_lap_us
 * @property {[number, number] | null} best_consecutive  [sum µs, first lap]
 * @property {number[]} ignored
 *
 * @typedef {object} SessionDetail
 * @property {Session} session
 * @property {PilotResult[]} results
 * @property {Pass[]} [passes]
 *
 * @typedef {object} LinkStats
 * @property {number} crc_errors
 * @property {number} lost
 * @property {number} clock_uncertainty_us
 * @property {number | null} drift_ppm
 *
 * @typedef {object} Board
 * @property {string} port
 * @property {string} info
 * @property {number} levels_per_s
 * @property {LinkStats | null} stats
 * @property {string | null} failed
 *
 * @typedef {object} State   the `state` message
 * @property {number} now_us
 * @property {Settings} settings
 * @property {Pilot[]} pilots
 * @property {Board[]} boards
 * @property {(SessionDetail & {passes: Pass[]}) | null} active
 *
 * @typedef {object} ChannelLive
 * @property {number | null} level_db
 * @property {number | null} enter_db
 * @property {number | null} exit_db
 * @property {boolean} in_pass
 *
 * @typedef {object} PilotStats
 * @property {number | null} pilot_id
 * @property {string} label
 * @property {number | null} best_lap_us
 * @property {number | null} best_consecutive_us
 * @property {number} sessions
 * @property {number} races
 * @property {number} wins
 *
 * @typedef {object} Stats  /api/stats
 * @property {number} today_races
 * @property {PilotStats[]} all
 * @property {PilotStats[]} today
 *
 * @typedef {object} Live    the `live` message, 10 times a second
 * @property {number} now_us
 * @property {ChannelLive[]} channels
 * @property {Board[]} boards
 */

export {}
