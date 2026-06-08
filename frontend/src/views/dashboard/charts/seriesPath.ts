/**
 * Shared SVG path builders for dashboard time-series visuals.
 * LineChart and SparklineChart both use the same monotone curve
 * and area-fill geometry so sparklines read as miniatures of the
 * full "Tickets over time" chart rather than a different dialect.
 */

export interface SeriesPoint {
  x: number
  y: number
}

export interface SparklinePaths {
  linePath: string
  areaPath: string
  priorPath: string
  last: SeriesPoint | null
}

/**
 * Monotone cubic Hermite path (Fritsch-Carlson). Produces a smooth
 * curve through the points that never overshoots them.
 */
export function monotonePath(pts: SeriesPoint[]): string {
  const n = pts.length
  if (n === 0) return ''
  if (n === 1) return `M${pts[0].x.toFixed(1)},${pts[0].y.toFixed(1)}`

  const dx: number[] = []
  const slope: number[] = []
  for (let i = 0; i < n - 1; i++) {
    const h = pts[i + 1].x - pts[i].x
    dx.push(h)
    slope.push(h === 0 ? 0 : (pts[i + 1].y - pts[i].y) / h)
  }

  const m: number[] = new Array(n)
  m[0] = slope[0]
  m[n - 1] = slope[n - 2]
  for (let i = 1; i < n - 1; i++) {
    m[i] = slope[i - 1] * slope[i] <= 0 ? 0 : (slope[i - 1] + slope[i]) / 2
  }
  for (let i = 0; i < n - 1; i++) {
    if (slope[i] === 0) {
      m[i] = 0
      m[i + 1] = 0
      continue
    }
    const a = m[i] / slope[i]
    const b = m[i + 1] / slope[i]
    const s = a * a + b * b
    if (s > 9) {
      const tau = 3 / Math.sqrt(s)
      m[i] = tau * a * slope[i]
      m[i + 1] = tau * b * slope[i]
    }
  }

  let d = `M${pts[0].x.toFixed(1)},${pts[0].y.toFixed(1)}`
  for (let i = 0; i < n - 1; i++) {
    const h = dx[i]
    const cp1x = pts[i].x + h / 3
    const cp1y = pts[i].y + (m[i] * h) / 3
    const cp2x = pts[i + 1].x - h / 3
    const cp2y = pts[i + 1].y - (m[i + 1] * h) / 3
    d += ` C${cp1x.toFixed(1)},${cp1y.toFixed(1)} ${cp2x.toFixed(1)},${cp2y.toFixed(1)} ${pts[i + 1].x.toFixed(1)},${pts[i + 1].y.toFixed(1)}`
  }
  return d
}

function valuesToPoints(
  values: number[],
  width: number,
  height: number,
  padLeft: number,
  padTop: number,
  padRight: number,
  padBottom: number,
  max: number,
): SeriesPoint[] {
  const innerW = Math.max(0, width - padLeft - padRight)
  const innerH = Math.max(0, height - padTop - padBottom)
  const step = values.length > 1 ? innerW / (values.length - 1) : 0
  return values.map((value, i) => ({
    x: padLeft + i * step,
    y: padTop + innerH - (value / max) * innerH,
  }))
}

/** Build line, area, and optional prior paths for a compact sparkline. */
export function buildSparklinePaths(opts: {
  values: number[]
  priorValues?: number[] | null
  width: number
  height: number
  padTop?: number
  padBottom?: number
  padLeft?: number
  padRight?: number
}): SparklinePaths | null {
  const {
    values,
    priorValues,
    width,
    height,
    padTop = 4,
    padBottom = 2,
    padLeft = 2,
    padRight = 4,
  } = opts

  if (values.length === 0 || width <= 0 || height <= 0) return null

  const prior = priorValues ?? []
  const max = Math.max(...values, ...prior, 1)
  const baselineY = height - padBottom

  const pts = valuesToPoints(values, width, height, padLeft, padTop, padRight, padBottom, max)
  const linePath = monotonePath(pts)
  const areaPath =
    pts.length > 0
      ? `${linePath} L${pts[pts.length - 1].x.toFixed(1)},${baselineY.toFixed(1)} L${pts[0].x.toFixed(1)},${baselineY.toFixed(1)} Z`
      : ''

  const priorPts =
    prior.length > 0
      ? valuesToPoints(prior, width, height, padLeft, padTop, padRight, padBottom, max)
      : []

  return {
    linePath,
    areaPath,
    priorPath: priorPts.length > 0 ? monotonePath(priorPts) : '',
    last: pts.length > 0 ? pts[pts.length - 1] : null,
  }
}
