// @ts-check
// Writes the firmware staged with the page (tools/fetch_firmware.sh puts it
// in /firmware/) to a board on this computer's USB, over Web Serial, with
// esptool-js. Only where the browser offers Web Serial: Chrome or Edge, and
// only on a "secure" page (localhost or HTTPS), so not from the Pi's
// http://… address; there the server flashes (Boards → Firmware).
// GPL-3.0-or-later. Derived from widscan58's web/src/serial/flasher.ts
// (CapnBry/widscan58). The firmware repository's page has the same file
// (fpv-laptimer-firmware, web/src/flasher.js).

import SparkMD5 from 'spark-md5'

/**
 * @typedef {{path: string, offset: number, size: number, sha256: string}} ManifestPart
 * @typedef {object} Manifest   ESP Web Tools' format, plus size and sha256 per part
 * @property {string} name
 * @property {string} version    as the firmware's INFO reports it
 * @property {string | null} [date]
 * @property {{chipFamily: string, parts: ManifestPart[]}[]} builds
 *
 * @typedef {object} FlashEvents
 * @property {(text: string) => void} stage
 * @property {(fraction: number) => void} progress
 * @property {(line: string) => void} log
 */

const FIRMWARE_DIR = '/firmware/'
const ESPRESSIF_VID = 0x303a

/** The ESP32-C5's LP watchdog registers (esptool's targets/esp32c5.py).
 * esptool-js 0.7 neither disables them on connect nor implements a watchdog
 * reset for the C5. */
const WDT = {
  config0: 0x600b1c00,
  config1: 0x600b1c04,
  protect: 0x600b1c18,
  key: 0x50d83aa1,
  swdConfig: 0x600b1c1c,
  swdProtect: 0x600b1c20,
  swdKey: 0x50d83aa1,
  swdAutoFeed: 1 << 18,
}

/** True when this browser can flash a board on this computer. */
export function webSerialAvailable() {
  return 'serial' in navigator
}

/** Shows the browser's port chooser (Espressif boards only). Call it from a
 * click: the browser refuses it otherwise. */
export function requestPort() {
  return navigator.serial.requestPort({ filters: [{ usbVendorId: ESPRESSIF_VID }] })
}

/** @returns {Promise<Manifest>} */
export async function loadManifest() {
  const r = await fetch(FIRMWARE_DIR + 'manifest.json', { cache: 'no-cache' })
  // The dev server answers a missing file with index.html, hence the JSON check.
  const m = r.ok ? /** @type {Manifest | null} */ (await r.json().catch(() => null)) : null
  if (!m?.builds) throw new Error('Mit dieser Seite ist keine Firmware bereitgestellt (tools/fetch_firmware.sh).')
  return m
}

/** @param {ManifestPart} p */
async function fetchPart(p) {
  const r = await fetch(FIRMWARE_DIR + p.path, { cache: 'no-cache' })
  if (!r.ok) throw new Error(`${p.path}: HTTP ${r.status}`)
  const data = new Uint8Array(await r.arrayBuffer())
  const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', data))
  const hex = Array.from(hash, (b) => b.toString(16).padStart(2, '0')).join('')
  if (data.length !== p.size || hex !== p.sha256) throw new Error(`${p.path}: Download beschädigt (sha256 passt nicht)`)
  return data
}

/**
 * Flashes `port` (closed; it is opened here and closed again). The board may
 * already be in download mode (BOOT + RESET); otherwise it is reset into it
 * over USB. Ends with a watchdog reset into the new firmware.
 * @param {SerialPort} port @param {Manifest} manifest @param {FlashEvents} ev
 */
export async function flashFirmware(port, manifest, ev) {
  // Loaded only when needed: esptool-js and its chip stubs are large.
  const { ESPLoader, Transport } = await import('esptool-js')
  // esptool-js writes "Connecting..." and its dots without a newline.
  let pending = ''
  const terminal = {
    clean() {},
    /** @param {string} s */
    write(s) {
      pending += s
    },
    /** @param {string} s */
    writeLine(s) {
      ev.log(pending + s)
      pending = ''
    },
  }
  const transport = new Transport(port)
  const loader = new ESPLoader({ transport, baudrate: 115200, terminal })
  try {
    ev.stage('Mit dem Bootloader verbinden …')
    try {
      // A board put into download mode by hand answers without a reset.
      await loader.main('no_reset')
    } catch {
      ev.log('Keine Antwort; Board über USB in den Download-Modus setzen')
      await transport.disconnect().catch(() => {})
      await loader.main('default_reset')
    }
    const chip = loader.chip.CHIP_NAME
    const build = manifest.builds.find((b) => b.chipFamily === chip)
    if (chip !== 'ESP32-C5' || !build) throw new Error(`Für den ${chip} gibt es hier keine Firmware.`)
    await disableWatchdogs(loader)

    ev.stage('Firmware laden …')
    const files = await Promise.all(build.parts.map(fetchPart))

    ev.stage(`Schreibe ${manifest.version} …`)
    const total = build.parts.reduce((s, p) => s + p.size, 0)
    const before = build.parts.map((_, i) => build.parts.slice(0, i).reduce((s, p) => s + p.size, 0))
    await loader.writeFlash({
      fileArray: files.map((data, i) => ({ data, address: build.parts[i].offset })),
      // The bootloader header already carries the right flash settings.
      flashMode: 'keep',
      flashFreq: 'keep',
      flashSize: 'keep',
      eraseAll: false,
      compress: true,
      reportProgress: (i, written, size) => ev.progress((before[i] + (build.parts[i].size * written) / size) / total),
      calculateMD5Hash: (image) => SparkMD5.ArrayBuffer.hash(image.slice().buffer),
    })
    ev.progress(1)

    ev.stage('Board neu starten …')
    await watchdogReset(loader)
  } finally {
    await transport.disconnect().catch(() => {})
  }
}

/** USB Serial/JTAG does not reset the RTC watchdogs on entering download
 * mode, and a watchdog the bootloader armed can reset the chip mid-flash.
 * @param {import('esptool-js').ESPLoader} loader */
async function disableWatchdogs(loader) {
  await loader.writeReg(WDT.protect, WDT.key)
  await loader.writeReg(WDT.config0, 0)
  await loader.writeReg(WDT.protect, 0)
  await loader.writeReg(WDT.swdProtect, WDT.swdKey)
  await loader.writeReg(WDT.swdConfig, ((await loader.readReg(WDT.swdConfig)) | WDT.swdAutoFeed) >>> 0)
  await loader.writeReg(WDT.swdProtect, 0)
}

/** A reset over USB leaves the C5 in download mode; the RTC watchdog's
 * system reset boots the app (esptool --after watchdog-reset).
 * @param {import('esptool-js').ESPLoader} loader */
async function watchdogReset(loader) {
  await loader.writeReg(WDT.protect, WDT.key)
  await loader.writeReg(WDT.config1, 2000)
  await loader.writeReg(WDT.config0, ((1 << 31) | (5 << 28) | (1 << 8) | 2) >>> 0)
  // The reset may land before this reply does.
  await loader.writeReg(WDT.protect, 0).catch(() => {})
  await new Promise((r) => setTimeout(r, 500))
}
