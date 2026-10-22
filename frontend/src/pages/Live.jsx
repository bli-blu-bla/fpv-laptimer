// @ts-check
// The live page: start a training or race, follow it, and afterwards look
// at the result and correct it. Slot settings sit behind each slot's gear,
// the race format behind the gear next to "Rennen starten".

import { useEffect, useRef, useState } from 'react'
import { ArrowCounterClockwise, FlagCheckered, GearSix, Infinity as InfinityIcon, Stop, X } from '@phosphor-icons/react'
import { api, serverNow } from '../api.js'
import { clock, endRule, sessionKind, START_MODES } from '../format.js'
import { slotsOf, standings } from '../model.js'
import CorrectDialog from '../components/CorrectDialog.jsx'
import DayBest from '../components/DayBest.jsx'
import Dialog from '../components/Dialog.jsx'
import Lane from '../components/Lane.jsx'
import PassLog from '../components/PassLog.jsx'
import RaceDialog from '../components/RaceDialog.jsx'
import SlotDialog from '../components/SlotDialog.jsx'

/** Seconds of signal history in the slot cards (10 points a second). */
const SPARK_POINTS = 150

/**
 * @param {object} props
 * @param {ReturnType<typeof import('../api.js').useServer>} props.server
 * @param {import('../types').State} props.state
 * @param {import('../types').Stats | null} props.stats
 * @param {() => void} props.onShowSessions
 */
export default function Live({ server, state, stats, onShowSessions }) {
  const { live, clockOffset, changed } = server
  const active = state.active
  const [error, setError] = useState('')
  const [slotDialog, setSlotDialog] = useState(/** @type {number | null} */ (null))
  const [raceDialog, setRaceDialog] = useState(false)
  const [correct, setCorrect] = useState(/** @type {number | null} */ (null))
  const [confirmDiscard, setConfirmDiscard] = useState(false)
  // The session that just ended stays on the page until the next start or "Schließen".
  const [doneId, setDoneId] = useState(/** @type {number | null} */ (null))
  const [done, setDone] = useState(/** @type {import('../types').SessionDetail | null} */ (null))
  const now = useNow(clockOffset, active != null)
  const spark = useSpark(live)

  const activeId = active?.session.id ?? null
  const lastActive = useRef(/** @type {number | null} */ (null))
  useEffect(() => {
    if (activeId != null) {
      lastActive.current = activeId
      setDoneId(null)
    } else if (lastActive.current != null) {
      setDoneId(lastActive.current) // it just ended
      lastActive.current = null
    }
  }, [activeId])

  const reloadDone = () => {
    if (doneId != null) api(`/sessions/${doneId}`).then(setDone).catch(() => setDoneId(null))
  }
  useEffect(() => {
    if (doneId == null) setDone(null)
    else reloadDone()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [doneId, changed])

  /** @param {() => Promise<unknown>} action */
  async function run(action) {
    setError('')
    try {
      await action()
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    }
  }
  /** @param {boolean} race */
  const start = (race) => run(() => api('/session/start', { body: { race } }))

  // What is shown: the running session, the one that just ended, or nothing.
  const detail = active ?? done
  const session = detail?.session
  const phase = active
    ? active.session.config.race && now < active.session.start_us
      ? 'countdown'
      : 'running'
    : done
      ? 'done'
      : 'idle'
  const t0 = session ? (session.config.race ? session.start_us : session.created_us) : 0
  const elapsed = phase === 'running' ? now - t0 : phase === 'done' && session ? (session.ended_us ?? t0) - t0 : 0
  const st = detail ? standings(detail.session, detail.results) : []

  const r = state.settings.race
  const formatShort = `${START_MODES[r.start_mode]} · ${endRule(r.end_rule)}${r.countdown_s ? ` · ${r.countdown_s} s Countdown` : ''}`
  const chip = {
    idle: ['Bereit', ''],
    countdown: ['Countdown', 'live'],
    running: [session?.config.race ? 'Läuft' : 'Training', 'live'],
    done: ['Beendet', 'done'],
  }[phase]

  const startButtons = (
    <>
      <button className="btn btn-secondary btn-big" onClick={() => start(false)}>
        <InfinityIcon size={20} /> Training
      </button>
      <span className="split">
        <button className="btn btn-primary btn-big btn-glow" onClick={() => start(true)}>
          <FlagCheckered size={20} />
          <span className="btn-stack">
            <span>Rennen starten</span>
            <span className="sub">{formatShort}</span>
          </span>
        </button>
        <button className="btn btn-primary btn-big btn-icon" style={{ width: 52 }} onClick={() => setRaceDialog(true)} title="Rennformat">
          <GearSix size={20} />
        </button>
      </span>
    </>
  )

  return (
    <>
      <div className="race-head">
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6, minWidth: 180 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
            <span className={'chip ' + chip[1]}>
              <span className="blink" />
              {chip[0]}
            </span>
            <span className="muted small">{session ? sessionKind(session.config) : formatShort}</span>
          </div>
          <div className="clock">{clock(elapsed)}</div>
        </div>
        <div className="actions">
          {(phase === 'idle' || phase === 'done') && startButtons}
          {phase === 'done' && (
            <>
              <button className="btn btn-ghost btn-big" onClick={() => setConfirmDiscard(true)}>
                <ArrowCounterClockwise size={18} /> Verwerfen
              </button>
              <button className="btn btn-secondary btn-big" onClick={() => setDoneId(null)}>
                <X size={18} /> Schließen
              </button>
            </>
          )}
          {(phase === 'running' || phase === 'countdown') && (
            <>
              <button className="btn btn-ghost btn-big" onClick={() => setConfirmDiscard(true)}>
                <X size={18} /> Abbrechen
              </button>
              <button className="btn btn-primary btn-big" onClick={() => run(() => api('/session/stop', { body: {} }))}>
                <Stop size={20} /> Stopp
              </button>
            </>
          )}
        </div>
      </div>
      {error && <p className="error">{error}</p>}

      <div className="lanes">
        {slotsOf(state).map((slot) => {
          const i = session ? session.seats.findIndex((x) => x.channel === slot.index) : -1
          const seat = i >= 0 ? session?.seats[i] : undefined
          const off = session ? !seat : !slot.enabled
          return (
            <Lane
              key={slot.index}
              color={slot.color}
              callsign={seat?.label ?? slot.callsign}
              fullName={seat ? (state.pilots.find((p) => p.id === seat.pilot_id)?.full_name ?? '') : slot.fullName}
              channel={seat?.channel_name ?? slot.channel}
              mhz={seat?.mhz ?? slot.mhz}
              off={off}
              result={i >= 0 ? detail?.results[i] : undefined}
              standing={i >= 0 ? st[i] : undefined}
              endRule={session?.config.race ? session.config.end_rule : undefined}
              live={live?.channels[slot.index]}
              spark={spark[slot.index] ?? []}
              onGear={() => setSlotDialog(slot.index)}
              onCorrect={phase === 'done' && seat ? () => setCorrect(seat.channel) : undefined}
            />
          )
        })}
      </div>

      <div className="two-col">
        <PassLog detail={detail} onChanged={reloadDone} />
        <DayBest stats={stats} onAll={onShowSessions} />
      </div>

      {phase === 'countdown' && active && (
        <div className="countdown">
          <div>
            <span className="kicker" style={{ fontSize: 14 }}>
              Startbereit machen
            </span>
            <span className="n">{Math.max(1, Math.ceil((active.session.start_us - now) / 1e6))}</span>
            <button className="btn btn-ghost btn-big" onClick={() => setConfirmDiscard(true)}>
              Abbrechen
            </button>
          </div>
        </div>
      )}

      {slotDialog != null && <SlotDialog state={state} stats={stats} index={slotDialog} color={slotsOf(state)[slotDialog].color} onClose={() => setSlotDialog(null)} />}
      {raceDialog && <RaceDialog state={state} onClose={() => setRaceDialog(false)} />}
      {correct != null && done && <CorrectDialog detail={done} channel={correct} onChanged={reloadDone} onClose={() => setCorrect(null)} />}
      {confirmDiscard && session && (
        <Dialog title={active ? 'Abbrechen?' : 'Verwerfen?'} onClose={() => setConfirmDiscard(false)}>
          <p className="muted" style={{ margin: 0 }}>
            {active ? 'Die Session wird gestoppt und mit allen Runden gelöscht.' : 'Die Session wird mit allen Runden gelöscht.'}
          </p>
          <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
            <button className="btn btn-secondary" onClick={() => setConfirmDiscard(false)}>
              Behalten
            </button>
            <button
              className="btn btn-danger"
              onClick={() =>
                run(async () => {
                  const id = session.id
                  if (active) await api('/session/stop', { body: {} })
                  await api(`/sessions/${id}`, { method: 'DELETE' })
                  lastActive.current = null
                  setDoneId(null)
                  setConfirmDiscard(false)
                })
              }
            >
              Löschen
            </button>
          </div>
        </Dialog>
      )}
    </>
  )
}

/** The server time, updated 10 times a second while `running`.
 * @param {{current: number}} clockOffset @param {boolean} running */
function useNow(clockOffset, running) {
  const [now, setNow] = useState(() => serverNow(clockOffset))
  useEffect(() => {
    if (!running) return
    const id = setInterval(() => setNow(serverNow(clockOffset)), 100)
    return () => clearInterval(id)
  }, [clockOffset, running])
  return now
}

/** The last seconds of every channel's live level, for the sparklines.
 * @param {import('../types').Live | null} live */
function useSpark(live) {
  const hist = useRef(/** @type {import('../components/Sparkline.jsx').SparkPoint[][]} */ ([]))
  const [, setTick] = useState(0)
  useEffect(() => {
    if (!live) return
    live.channels.forEach((c, i) => {
      const h = (hist.current[i] ??= [])
      h.push({ level: c.level_db, enter: c.enter_db, exit: c.exit_db })
      if (h.length > SPARK_POINTS) h.shift()
    })
    setTick((t) => t + 1)
  }, [live])
  return hist.current
}
