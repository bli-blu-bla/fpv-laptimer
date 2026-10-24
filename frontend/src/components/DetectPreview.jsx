// @ts-check
// Settings → Messung: a made-up signal showing what the minimum swing does.
// Two passes rise ~22 and ~18 dB above the course level, a fly-over ~10 dB:
// it counts only when the swing is set lower than that. Only an illustration: the real
// thresholds are learnt from the flight (detect.rs).

/** @param {{riseDb: number}} props */
export default function DetectPreview({ riseDb }) {
  const H = 96
  const base = 30
  const thr = base + riseDb * 2.2
  // Course noise plus three bumps: [centre, width, height]; 2.2 units per dB.
  const bumps = [
    [22, 9, 48],
    [64, 9, 40],
    [98, 6, 22],
  ]
  /** @type {[number, number][]} */
  const pts = []
  for (let i = 0; i <= 120; i++) {
    let v = base + Math.sin(i * 1.3) * 3 + Math.sin(i * 0.37) * 2
    for (const [c, w, h] of bumps) v += h * Math.exp(-((i - c) ** 2) / (2 * (w / 2.2) ** 2))
    pts.push([(i / 120) * 100, v])
  }
  /** @type {[number, number][]} */
  const counted = []
  let start = /** @type {number | null} */ (null)
  pts.forEach(([px, v], i) => {
    if (v >= thr && start == null) start = px
    if ((v < thr || i === 120) && start != null) {
      counted.push([start, px])
      start = null
    }
  })
  const y = (/** @type {number} */ v) => H - v
  return (
    <div className="detect-preview">
      <svg viewBox={`0 0 100 ${H}`} preserveAspectRatio="none">
        {counted.map(([a, b], k) => (
          <rect key={k} x={a} y={0} width={b - a} height={H} className="counted" />
        ))}
        <line x1={0} x2={100} y1={y(base)} y2={y(base)} className="course" />
        <line x1={0} x2={100} y1={y(thr)} y2={y(thr)} className="thr" />
        <polyline points={pts.map(([px, v]) => `${px.toFixed(2)},${y(v).toFixed(1)}`).join(' ')} className="sig" />
      </svg>
    </div>
  )
}
