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
 * @property {number | null} [preset_id]  the pilot's preset last copied into this channel's detection
 *
 * @typedef {object} Settings
 * @property {ChannelSetting[]} channels
 * @property {SeatSetting[]} seats
 * @property {number} gain
 * @property {{start_mode: StartMode, end_rule: EndRule, countdown_s: number}} race
 * @property {number} min_lap_s
 * @property {number} consecutive
 * @property {boolean} sound
 * @property {AnnounceSettings} announce
 * @property {DetectionSettings} detection
 * @property {Record<string, BoardRole>} board_roles   by board id; missing: finish
 *
 * @typedef {'off' | 'beep' | 'number' | 'time'} LapCall   number: "<pilot> ist in Runde 3", time: "<pilot> 13,7"
 * @typedef {object} AnnounceSettings
 * @property {'server' | 'browser'} output   browser: played by browsers switched on as speaker
 * @property {boolean} start      countdown and start tone; "Training läuft"
 * @property {LapCall} lap
 * @property {boolean} best       training: "Pilot <name> hat eine neue Bestzeit erzielt: 13,7"
 * @property {boolean} finished   race: "<name> ist im Ziel"
 * @property {boolean} race_over  race: "Rennen beendet"
 *
 * @typedef {'finish' | 'arming' | 'off'} BoardRole
 * @typedef {'auto' | 'classic'} DetectMode
 * @typedef {{enter_db: number, exit_db: number, mode?: DetectMode | null, min_rise_db?: number | null}} Thresholds   mode / min_rise_db: none = the gate's / the common one
 * @typedef {object} DetectionSettings
 * @property {DetectMode} finish_mode   for channels without their own mode
 * @property {DetectMode} arming_mode
 * @property {Thresholds[]} finish   classic: per channel
 * @property {Thresholds[]} arming
 * @property {number} min_rise_db   automatic mode: minimum swing above the course, dB (channels without their own)
 *
 * @typedef {object} Pilot
 * @property {number} id
 * @property {string} name       the callsign
 * @property {string} full_name  may be empty
 *
 * @typedef {object} Preset   a pilot's finish-gate detection for one quad
 * @property {number} id
 * @property {number} pilot_id
 * @property {string} name
 * @property {DetectMode} mode
 * @property {number} enter_db
 * @property {number} exit_db
 * @property {number} min_rise_db
 * @property {number} gain   the boards' gain when it was saved
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
 * @property {number | null} confirmed_us   result confirmed (races; null: not yet)
 * @property {Seat[]} seats
 *
 * @typedef {object} Pass
 * @property {number} id
 * @property {number} channel
 * @property {number} time_us
 * @property {number} peak_db
 * @property {boolean} manual
 * @property {boolean} deleted
 * @property {number} gate   0: finish gate, 1: arming gate
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
 * @property {string} id         USB serial number (MAC)
 * @property {string | null} name   saved on the board (`NAME`), null without one
 * @property {BoardRole} role
 * @property {number | null} level_db
 * @property {string} info
 * @property {number} levels_per_s
 * @property {LinkStats | null} stats
 * @property {string | null} failed
 *
 * @typedef {object} State   the `state` message
 * @property {number} now_us
 * @property {string} version   the server's version (release tag, or `git describe`)
 * @property {Settings} settings
 * @property {Pilot[]} pilots
 * @property {Preset[]} presets
 * @property {Board[]} boards
 * @property {(SessionDetail & {passes: Pass[]}) | null} active
 * @property {number | null} unconfirmed   the ended race whose result still needs confirming (blocks starting)
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
 * @property {ChannelLive[]} channels   the finish gate
 * @property {ChannelLive[] | null} arming   the arming gate, if a board watches it
 * @property {Board[]} boards
 *
 * @typedef {object} FlashStatus   flashing over the server (the `flash` message)
 * @property {string} port
 * @property {string} stage
 * @property {number | null} progress   0–1 while writing
 * @property {boolean} done
 * @property {string | null} error
 *
 * @typedef {object} FirmwarePort
 * @property {string} port
 * @property {string} id
 * @property {string | null} info   the board's INFO; null: not running our firmware, or not set up
 * @property {string | null} name   the name saved on the board
 *
 * @typedef {object} FirmwareState   /api/firmware
 * @property {import('./flasher.js').Manifest | null} firmware   the firmware on offer
 * @property {string | null} error   why there is none
 * @property {FirmwarePort[]} ports
 * @property {FlashStatus | null} flash
 *
 * @typedef {'newer' | 'same' | 'older' | 'unclear'} VersionOrder   compared with the running version
 *
 * @typedef {object} UpdateStatus   /api/update (server/src/update.rs)
 * @property {string} version
 * @property {boolean} installed   runs from an installed release (only then updates)
 * @property {string | null} previous   the version "back" goes to
 * @property {boolean} online   there is an update source ([update] url, default GitHub)
 * @property {'GitHub' | 'Forgejo' | null} source   where "search for updates" asks
 * @property {string} platform   e.g. linux-aarch64: the package to pick
 * @property {{version: string, order: VersionOrder, source: string} | null} staged   checked, waiting for "install"
 * @property {{to: string, from: string, time_s: number | null} | null} failed   an update that didn't come up (rolled back)
 * @property {boolean} pending   a new version still has to prove it runs
 * @property {boolean} managed   runs as a service (systemd, launchd): starts at boot and again after an exit
 * @property {boolean} power   may reboot / shut down the Pi
 *
 * @typedef {object} RemoteStatus   the connection to a server (server/src/remote.rs)
 * @property {'off' | 'connecting' | 'connected' | 'waiting'} state   waiting: for the next attempt
 * @property {number | null} since_us   since when, µs since 1970
 * @property {string | null} error   why the last attempt or connection failed
 * @property {number} sent   messages sent since the server started
 * @property {number} buffered   events waiting for the connection
 * @property {number} dropped   events dropped (too old, too many)
 *
 * @typedef {object} RemoteView   /api/remote (needs the login, also for reading)
 * @property {boolean} enabled
 * @property {string} url   ws://… or wss://…
 * @property {boolean} authorization_set   the Authorization header of the handshake is set (never comes back)
 * @property {boolean} auth_token_set   the token sent as first message is set (never comes back)
 * @property {boolean} commands   the server may send commands
 * @property {RemoteStatus} status
 *
 * @typedef {object} OnlineRelease   /api/update/releases
 * @property {string} tag
 * @property {string} version
 * @property {boolean} prerelease
 * @property {string} published_at
 * @property {VersionOrder} order
 * @property {boolean} available   has the package for this processor and its signature
 */

export {}
