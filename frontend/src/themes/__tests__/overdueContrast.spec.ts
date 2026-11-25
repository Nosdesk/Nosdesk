import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import { themeRegistry } from '@/themes'
import { lightTheme } from '@/themes/base/light'
import { darkTheme } from '@/themes/base/dark'
import { overdueColor } from '@/themes/utils/cssInjector'

type Rgb = [number, number, number]

/** `#rrggbb` or `rgba(r, g, b, a)`, the latter laid over `under`. */
function parse(colour: string, under?: Rgb): Rgb {
  const hex = /^#([0-9a-f]{6})$/i.exec(colour)
  if (hex) return [0, 2, 4].map((i) => parseInt(hex[1].slice(i, i + 2), 16) / 255) as Rgb
  const rgba = /^rgba?\(([^)]+)\)$/.exec(colour)
  if (!rgba) throw new Error(`unreadable colour ${colour}`)
  const [r, g, b, a = 1] = rgba[1].split(',').map((v) => Number(v.trim()))
  const top = [r / 255, g / 255, b / 255]
  const base = under ?? [1, 1, 1]
  return top.map((v, i) => v * a + base[i] * (1 - a)) as Rgb
}

function luminance([r, g, b]: Rgb): number {
  const f = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4)
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}

function contrast(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (hi + 0.05) / (lo + 0.05)
}

describe('the overdue text colour', () => {
  it.each([...themeRegistry.entries()])('reaches 4.5:1 on the surface and the hover fill in %s', (_id, theme) => {
    const surface = parse(theme.colors.surface)
    const hover = parse(theme.colors.surfaceHover, surface)
    const overdue = parse(overdueColor(theme))
    expect(contrast(overdue, surface)).toBeGreaterThanOrEqual(4.5)
    expect(contrast(overdue, hover)).toBeGreaterThanOrEqual(4.5)
  })

  it('falls back to the base light or dark value, which the stylesheet defaults to as well', () => {
    const css = readFileSync(resolve(__dirname, '../../assets/main.css'), 'utf8')
    const values = [...css.matchAll(/--color-overdue: (#[0-9a-f]{6});/gi)].map((m) => m[1].toLowerCase())
    expect(values).toEqual([lightTheme.colors.overdue, lightTheme.colors.overdue, darkTheme.colors.overdue])
    expect(overdueColor({ ...darkTheme, colors: { ...darkTheme.colors, overdue: undefined } })).toBe(darkTheme.colors.overdue)
  })
})
