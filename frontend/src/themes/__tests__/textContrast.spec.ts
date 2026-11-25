import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import { themeRegistry, type Theme } from '@/themes'
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

/** The fills text sits on. A translucent one is laid over the surface,
 *  which is where rows and cards draw it. */
function backgrounds(theme: Theme): Record<string, Rgb> {
  const surface = parse(theme.colors.surface)
  return {
    app: parse(theme.colors.app),
    surface,
    surfaceAlt: parse(theme.colors.surfaceAlt, surface),
    surfaceHover: parse(theme.colors.surfaceHover, surface),
  }
}

const TEXT = ['primary', 'secondary', 'tertiary'] as const
const themes = [...themeRegistry.entries()]
const css = readFileSync(resolve(__dirname, '../../assets/main.css'), 'utf8')

// WCAG 1.4.3 asks 4.5:1 for normal text, unrounded. Most secondary and
// tertiary text is 11 to 13px, so no theme gets the large-text 3:1.
describe('text colours', () => {
  const cases = themes.flatMap(([id, theme]) =>
    TEXT.flatMap((token) =>
      Object.entries(backgrounds(theme)).map(([on, background]) => ({ id, token, on, theme, background })),
    ),
  )

  it.each(cases)('$id: $token reaches 4.5:1 on $on', ({ theme, token, background }) => {
    expect(contrast(parse(theme.colors[token]), background)).toBeGreaterThanOrEqual(4.5)
  })

  it.each(themes)('%s: primary, secondary and tertiary keep their order on the surface', (_id, theme) => {
    const surface = parse(theme.colors.surface)
    const [primary, secondary, tertiary] = TEXT.map((token) => contrast(parse(theme.colors[token]), surface))
    expect(primary).toBeGreaterThan(secondary)
    expect(secondary).toBeGreaterThan(tertiary)
  })

  // A hovered row has to read as hovered on a plain row and on an
  // alternate one. 37 controls fill with surface-alt and hover to
  // surface-hover, so both distances matter.
  it.each(themes)('%s: the hover fill stands 1.15:1 apart from the surface and the alternate surface', (_id, theme) => {
    const { surface, surfaceAlt, surfaceHover } = backgrounds(theme)
    expect(contrast(surfaceHover, surface)).toBeGreaterThanOrEqual(1.15)
    expect(contrast(surfaceHover, surfaceAlt)).toBeGreaterThanOrEqual(1.15)
  })

  // main.css paints the first frame before the theme store injects its
  // variables: `@theme` and `:root` carry the light values, `.dark` the
  // dark ones.
  it.each([...TEXT, 'surfaceHover'] as const)('%s in the stylesheet defaults is the base light and dark value', (token) => {
    const name = token.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)
    const values = [...css.matchAll(new RegExp(`--color-${name}: (#[0-9a-f]{6});`, 'gi'))].map((m) => m[1].toLowerCase())
    expect(values).toEqual([lightTheme.colors[token], lightTheme.colors[token], darkTheme.colors[token]])
  })
})

describe('the overdue text colour', () => {
  it.each(themes)('reaches 4.5:1 on the surface and the hover fill in %s', (_id, theme) => {
    const { surface, surfaceHover } = backgrounds(theme)
    const overdue = parse(overdueColor(theme))
    expect(contrast(overdue, surface)).toBeGreaterThanOrEqual(4.5)
    expect(contrast(overdue, surfaceHover)).toBeGreaterThanOrEqual(4.5)
  })

  it('falls back to the base light or dark value, which the stylesheet defaults to as well', () => {
    const values = [...css.matchAll(/--color-overdue: (#[0-9a-f]{6});/gi)].map((m) => m[1].toLowerCase())
    expect(values).toEqual([lightTheme.colors.overdue, lightTheme.colors.overdue, darkTheme.colors.overdue])
    expect(overdueColor({ ...darkTheme, colors: { ...darkTheme.colors, overdue: undefined } })).toBe(darkTheme.colors.overdue)
  })
})
