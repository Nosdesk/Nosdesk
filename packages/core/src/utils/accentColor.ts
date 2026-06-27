/** Preset S/L for accent colours — shared with ColorHueSlider. */
export const ACCENT_SATURATION = 70
export const ACCENT_LIGHTNESS = 45

export function hslToHex(h: number, s: number, l: number): string {
  s /= 100
  l /= 100

  const c = (1 - Math.abs(2 * l - 1)) * s
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1))
  const m = l - c / 2

  let r = 0
  let g = 0
  let b = 0
  if (h < 60) { r = c; g = x }
  else if (h < 120) { r = x; g = c }
  else if (h < 180) { g = c; b = x }
  else if (h < 240) { g = x; b = c }
  else if (h < 300) { r = x; b = c }
  else { r = c; b = x }

  const toHex = (n: number) => Math.round((n + m) * 255).toString(16).padStart(2, '0')
  return `#${toHex(r)}${toHex(g)}${toHex(b)}`
}

/** Random hue (0–359) at the standard accent saturation and lightness. */
export function randomAccentHue(): number {
  return Math.floor(Math.random() * 360)
}

export function accentColorFromHue(hue: number): string {
  return hslToHex(hue, ACCENT_SATURATION, ACCENT_LIGHTNESS)
}

/** Default colour for new collections — hue only is randomised. */
export function randomAccentColor(): string {
  return accentColorFromHue(randomAccentHue())
}
