// @ts-check
// Ein Rennen über 3 Runden: Piloten auf die Kanäle, starten, jede Runde
// ausgeben, am Ende das Ergebnis bestätigen.
//
//   LAPTIMER_URL=http://pi:5800 LAPTIMER_KEY=lt_… node examples/race.js NITRO ACE
//
// Ohne Boards: auf der Seite (oder per passes.add) Durchflüge eintragen.

import { Laptimer } from '../src/index.js'

const lt = new Laptimer({ url: process.env.LAPTIMER_URL ?? 'http://localhost:5800', key: process.env.LAPTIMER_KEY ?? '' })

lt.on('countdown', (d) => console.log(`… ${d.seconds_left}`))
lt.on('start_signal', () => console.log('Los!'))
lt.on('pilot_pass', (d) => {
  if (d.kind === 'lap') console.log(`${d.pilot}: Runde ${d.lap} in ${d.lap_time_s?.toFixed(2)} s${d.personal_best ? ' (Bestzeit)' : ''}, Platz ${d.position}`)
  else if (d.kind === 'holeshot') console.log(`${d.pilot}: Holeshot`)
})
lt.on('pilot_finished', (d) => console.log(`${d.pilot} ist im Ziel: Platz ${d.position}, ${d.total_s?.toFixed(2)} s`))

await lt.connect()

const pilots = process.argv.slice(2)
if (pilots.length) await lt.channels.assign(pilots, { create: true })

const session = await lt.session.start({ race: true, laps: 3, countdown_s: 5 })
console.log(`Rennen ${session.id} gestartet`)

// Das Rennen endet von selbst, wenn alle im Ziel sind (oder auf der Seite gestoppt).
const stopped = await lt.waitFor('session_stopped', { filter: (d) => d.session_id === session.id, timeout_ms: 30 * 60 * 1000 })
console.log(`Sieger: ${stopped.winner ?? '–'}`)
for (const r of stopped.results) console.log(`  ${r.position ?? '-'}. ${r.pilot}: ${r.laps} Runden`)

if (stopped.needs_confirmation) {
  await lt.session.confirm(session.id)
  console.log('Ergebnis bestätigt')
}
lt.close()
