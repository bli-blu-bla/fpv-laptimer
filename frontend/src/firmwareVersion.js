// @ts-check
// Firmware versions as INFO reports them: git describe of fpv-laptimer-firmware
// without the tag's "v" ("0.1.0", "0.1.0-3-gabc1234", "0.2.0-rc1",
// "0.1.0-dirty"). Older builds, from when the firmware lived in the laptimer
// repository, report the laptimer's describe ("v0.0.12-5-g…"); those count
// as older. A bare commit hash (no tag) can't be compared.
// The firmware repository's page has the same rules (fpv-laptimer-firmware,
// web/src/version.js).
// GPL-3.0-or-later.

/**
 * @typedef {{nums: number[], pre: string | null, ahead: number, dirty: boolean}} Version
 */

const PATTERN = /^v?(\d+)\.(\d+)\.(\d+)(?:-((?!\d+-g|dirty$)[0-9A-Za-z.]+))?(?:-(\d+)-g[0-9a-f]+)?(-dirty)?$/

/** @param {string | null | undefined} v @returns {Version | null} */
export function parseVersion(v) {
  const m = v?.trim().match(PATTERN)
  if (!m) return null
  return {
    nums: [Number(m[1]), Number(m[2]), Number(m[3])],
    pre: m[4] ?? null,
    ahead: m[5] ? Number(m[5]) : 0,
    dirty: !!m[6],
  }
}

/**
 * -1 when a is older than b, 0 the same, 1 newer, null when one of them is no
 * version (a commit hash, "unknown"). A release beats its pre-releases
 * (0.2.0 > 0.2.0-rc1); commits after a tag and a dirty tree are newer still.
 * @param {string | null | undefined} a @param {string | null | undefined} b
 * @returns {-1 | 0 | 1 | null}
 */
export function compareVersions(a, b) {
  const x = parseVersion(a)
  const y = parseVersion(b)
  if (!x || !y) return null
  /** @param {number} d @returns {-1 | 0 | 1} */
  const sign = (d) => (d < 0 ? -1 : d > 0 ? 1 : 0)
  for (let i = 0; i < 3; i++) if (x.nums[i] !== y.nums[i]) return sign(x.nums[i] - y.nums[i])
  if (x.pre !== y.pre) {
    if (x.pre == null) return 1
    if (y.pre == null) return -1
    return sign(x.pre.localeCompare(y.pre, 'en', { numeric: true }))
  }
  if (x.ahead !== y.ahead) return sign(x.ahead - y.ahead)
  return sign(Number(x.dirty) - Number(y.dirty))
}
