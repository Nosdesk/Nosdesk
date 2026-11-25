/**
 * Colours made readable on the fills they sit on.
 *
 * A status or accent colour is chosen to work as a fill or a dot: amber
 * for warning, a bright red for error, the brand orange. Used as text on
 * a light surface most of them fall under WCAG's 4.5:1 (amber on white
 * is under 2:1), and on their own 15% tint they do worse. `readableOn`
 * keeps the hue and moves only lightness (in OKLCH, so the hue holds as
 * the colour darkens or lightens) until the colour reaches the ratio on
 * every fill given, so a theme's text tokens are derived from its fill
 * colours rather than kept by hand, and a workspace's own accent gets
 * a readable text version too.
 */

export type Rgb = [number, number, number]

/** `#rrggbb` or `rgba(r, g, b, a)` as sRGB in 0..1, a translucent colour
 *  laid over `under` (white when not given). */
export function parseColour(colour: string, under: Rgb = [1, 1, 1]): Rgb {
  const hex = /^#([0-9a-f]{6})$/i.exec(colour.trim())
  if (hex) return [0, 2, 4].map((i) => parseInt(hex[1].slice(i, i + 2), 16) / 255) as Rgb
  const rgba = /^rgba?\(([^)]+)\)$/.exec(colour.trim())
  if (!rgba) throw new Error(`unreadable colour ${colour}`)
  const [r, g, b, a = 1] = rgba[1].split(',').map((v) => Number(v.trim()))
  return [r / 255, g / 255, b / 255].map((v, i) => v * a + under[i] * (1 - a)) as Rgb
}

export function toHex(rgb: Rgb): string {
  return `#${rgb.map((v) => Math.round(Math.min(1, Math.max(0, v)) * 255).toString(16).padStart(2, '0')).join('')}`
}

const toLinear = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4)
const toGamma = (v: number) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055)

/** WCAG 2.x relative luminance. */
export function luminance(rgb: Rgb): number {
  const [r, g, b] = rgb.map(toLinear)
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

/** WCAG 2.x contrast ratio, unrounded. */
export function contrast(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (hi + 0.05) / (lo + 0.05)
}

type Oklch = [number, number, number]

function toOklch(rgb: Rgb): Oklch {
  const [r, g, b] = rgb.map(toLinear)
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s
  return [L, Math.hypot(A, B), Math.atan2(B, A)]
}

function fromOklch([L, C, h]: Oklch): Rgb {
  const A = C * Math.cos(h)
  const B = C * Math.sin(h)
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ]
}

/** The colour at this OKLCH point, chroma eased off until it is in sRGB. */
function inGamut(L: number, C: number, h: number): Rgb {
  for (let c = C; c > 0; c -= 0.002) {
    const lin = fromOklch([L, c, h])
    if (lin.every((v) => v >= -1e-6 && v <= 1 + 1e-6)) return lin.map((v) => toGamma(Math.min(1, Math.max(0, v)))) as Rgb
  }
  return fromOklch([L, 0, h]).map((v) => toGamma(Math.min(1, Math.max(0, v)))) as Rgb
}

/**
 * `colour` with its lightness moved, hue kept, until it reaches `ratio`
 * against every one of `fills`. `lighten` picks the direction: true on
 * a dark theme, false on a light one. Returns the colour unchanged when
 * it already passes, and black or white when nothing short of them does.
 */
export function readableOn(colour: string, fills: Rgb[], ratio: number, lighten: boolean): string {
  const start = parseColour(colour)
  // Judged on the 8-bit colour that will be written, not the exact one.
  const passes = (hex: string) => fills.every((fill) => contrast(parseColour(hex), fill) >= ratio)
  if (passes(toHex(start))) return toHex(start)
  const [L, C, h] = toOklch(start)
  for (let step = 1; step <= 400; step++) {
    const next = lighten ? L + step * 0.0025 : L - step * 0.0025
    if (next < 0 || next > 1) break
    const hex = toHex(inGamut(next, C, h))
    if (passes(hex)) return hex
  }
  return lighten ? '#ffffff' : '#000000'
}
