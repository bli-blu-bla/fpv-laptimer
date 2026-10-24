// @ts-check
// The IMD rating as a coloured number (imd.js): green from 90, orange from
// 50, then amber, red below 0; "–" when there is nothing to rate.

/** @param {{rating: number | null}} props */
export default function ImdBadge({ rating }) {
  const cls = rating == null || rating >= 90 ? 'good' : rating >= 50 ? 'ok' : rating >= 0 ? 'warn' : 'bad'
  return (
    <span className={`imd-badge ${cls}`} title="IMD-Bewertung wie in RotorHazard (IMDTabler): 100 = keine Störungen, kleiner ist schlechter">
      <span className="k">IMD</span>
      <span className="v tnum">{rating ?? '–'}</span>
    </span>
  )
}
