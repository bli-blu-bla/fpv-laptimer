// @ts-check
// The last seconds of a channel's signal, with the detection thresholds.

/** The sparkline spans this range, dBFS. */
const MIN_DB = -62
const MAX_DB = -5

/**
 * @typedef {{level: number | null, enter: number | null, exit: number | null}} SparkPoint
 * @param {{points: SparkPoint[], color: string}} props
 */
export default function Sparkline({ points, color }) {
  const n = Math.max(points.length, 2)
  /** @param {number} db */
  const y = (db) => 40 - ((Math.min(MAX_DB, Math.max(MIN_DB, db)) - MIN_DB) / (MAX_DB - MIN_DB)) * 40
  const line = points
    .map((p, k) => (p.level == null ? null : `${((k / (n - 1)) * 100).toFixed(1)},${y(p.level).toFixed(1)}`))
    .filter(Boolean)
    .join(' ')
  const last = points.at(-1)
  return (
    <svg viewBox="0 0 100 40" preserveAspectRatio="none">
      {last?.enter != null && (
        <line x1={0} x2={100} y1={y(last.enter)} y2={y(last.enter)} stroke="#ff8a3d" strokeOpacity={0.6} strokeDasharray="2 2" vectorEffect="non-scaling-stroke" />
      )}
      {last?.exit != null && (
        <line x1={0} x2={100} y1={y(last.exit)} y2={y(last.exit)} stroke="#75798c" strokeDasharray="2 2" vectorEffect="non-scaling-stroke" />
      )}
      {line && (
        <>
          <polygon points={`0,40 ${line} 100,40`} fill={color} fillOpacity={0.12} />
          <polyline points={line} fill="none" stroke={color} strokeWidth={1.6} vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
        </>
      )}
    </svg>
  )
}
