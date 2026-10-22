// @ts-check
// Zeigt jedes Event des Laptimers an, bis Strg+C. Verbindet sich nach einem
// Abbruch (Pi neu gestartet, WLAN weg) von selbst wieder.
//
//   LAPTIMER_URL=http://pi:5800 LAPTIMER_KEY=lt_… node examples/events.js

import { Laptimer } from '../src/index.js'

const lt = new Laptimer({ url: process.env.LAPTIMER_URL ?? 'http://localhost:5800', key: process.env.LAPTIMER_KEY ?? '' })

lt.on('hello', (h) => console.log('verbunden, laufende Session:', h.state?.session?.id ?? 'keine'))
lt.on('disconnected', () => console.log('Verbindung weg, verbinde neu …'))
lt.on('lagged', (d) => console.log(`${d.missed} Events verpasst`))
lt.on('*', (data, msg) => console.log(`#${msg.seq} ${msg.event}`, JSON.stringify(data)))

try {
  await lt.connect()
} catch (e) {
  console.error(e instanceof Error ? e.message : e)
  process.exit(1)
}
