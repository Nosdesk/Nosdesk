import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import { themeRegistry, type Theme } from '@/themes'
import { lightTheme } from '@/themes/base/light'
import { darkTheme } from '@/themes/base/dark'
import { MERGED, overdueColor, resolveTheme, TEXT_COLOURS, UI_COLOURS } from '@/themes/utils/cssInjector'

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
    // A translucent text colour is laid over the background it is read on.
    expect(contrast(parse(theme.colors[token], background), background)).toBeGreaterThanOrEqual(4.5)
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
    expect(contrast(parse(overdueColor(theme), surface), surface)).toBeGreaterThanOrEqual(4.5)
    expect(contrast(parse(overdueColor(theme), surfaceHover), surfaceHover)).toBeGreaterThanOrEqual(4.5)
  })

  it('falls back to the base light or dark value, which the stylesheet defaults to as well', () => {
    const values = [...css.matchAll(/--color-overdue: (#[0-9a-f]{6});/gi)].map((m) => m[1].toLowerCase())
    expect(values).toEqual([lightTheme.colors.overdue, lightTheme.colors.overdue, darkTheme.colors.overdue])
    expect(overdueColor({ ...darkTheme, colors: { ...darkTheme.colors, overdue: undefined } })).toBe(darkTheme.colors.overdue)
  })
})

// Status, priority and accent colours are fills first. As text they read
// the derived `--color-<name>-text` (resolveTheme), which `text-status-*`,
// `text-priority-*` and `text-accent` resolve to.
describe('status, priority and accent text', () => {
  /** Every background, then (for text that sits on tints) the colour's
   *  muted token and the colour itself at 10, 15 and 20% over each of
   *  them: chips use `bg-status-error-muted`, and buttons and rows use
   *  `bg-status-error/20` on hover. */
  function fillsFor(colors: Theme['colors'], colour: string, tint: string | null): Array<[string, Rgb]> {
    const plain = Object.entries(backgrounds({ colors } as Theme))
    if (!tint) return plain
    const solid = parse(colour)
    return [
      ...plain,
      ...plain.flatMap(([on, under]) => [
        [`its muted tint over ${on}`, parse(tint, under)] as [string, Rgb],
        ...[0.1, 0.15, 0.2].map((a) => [`it at ${a * 100}% over ${on}`, solid.map((v, i) => v * a + under[i] * (1 - a)) as Rgb] as [string, Rgb]),
      ]),
    ]
  }

  const cases = themes.flatMap(([id, theme]) => {
    const { colors, text } = resolveTheme(theme)
    return TEXT_COLOURS.flatMap(([name, key, muted]) =>
      fillsFor(colors, colors[key] as string, muted && (colors[muted] as string)).map(([on, fill]) => ({ id, name, on, value: text[name], fill })),
    )
  })

  it.each(cases)('$id: text-$name reaches 4.5:1 on $on', ({ value, fill }) => {
    expect(contrast(parse(value), fill)).toBeGreaterThanOrEqual(4.5)
  })

  // Accent text sits on plain backgrounds, so it takes the lightest
  // colour that reads there (an orange, not a brown); one step lighter
  // in OKLCH would not.
  it.each(themes)('%s: accent text is no darker than it has to be', (_id, theme) => {
    const { colors, text } = resolveTheme(theme)
    const plain = Object.values(backgrounds({ colors } as Theme))
    const worst = Math.min(...plain.map((fill) => contrast(parse(text.accent), fill)))
    // Already readable as is, or within one 8-bit step of the line.
    expect(text.accent.toLowerCase() === colors.accent.toLowerCase() || worst < 4.75).toBe(true)
  })

  // Icons and glyphs that carry meaning: 3:1 (WCAG 1.4.11) on every
  // background and tint.
  const uiCases = themes.flatMap(([id, theme]) => {
    const { colors, ui } = resolveTheme(theme)
    return [
      ...UI_COLOURS.flatMap(([name, key, muted]) =>
        fillsFor(colors, colors[key] as string, colors[muted] as string).map(([on, fill]) => ({ id, name, on, value: ui[name], fill })),
      ),
      ...fillsFor(colors, (theme.meta.isDark ? MERGED.dark : MERGED.light).fill, (theme.meta.isDark ? MERGED.dark : MERGED.light).tint)
        .map(([on, fill]) => ({ id, name: 'status-merged', on, value: ui['status-merged'], fill })),
    ]
  })
  it.each(uiCases)('$id: text-$name-ui reaches 3:1 on $on', ({ value, fill }) => {
    expect(contrast(parse(value), fill)).toBeGreaterThanOrEqual(3)
  })

  it.each([...UI_COLOURS.map(([name]) => name), 'status-merged' as const])('--color-%s-ui in the stylesheet defaults is the base light and dark value', (name) => {
    const values = [...css.matchAll(new RegExp(`--color-${name}-ui: (#[0-9a-f]{6});`, 'gi'))].map((m) => m[1].toLowerCase())
    expect(values).toEqual([resolveTheme(lightTheme).ui[name], resolveTheme(darkTheme).ui[name]])
  })

  // A workspace's own accent replaces the theme's and gets a text
  // version of its own. (Pure white is left out: on a dark theme its
  // own 20% tint leaves no lighter colour to move to.)
  const accents = ['#FFD400', '#00C951', '#2C80FF', '#FF66B3', '#7C3AED', '#111111']
  it.each(themes.flatMap(([id, theme]) => accents.map((accent) => ({ id, theme, accent }))))(
    '$id with the workspace accent $accent: accent text reaches 4.5:1 everywhere it sits',
    ({ theme, accent }) => {
      const { colors, text } = resolveTheme(theme, accent)
      for (const [, fill] of fillsFor(colors, colors.accent, null)) {
        expect(contrast(parse(text.accent), fill)).toBeGreaterThanOrEqual(4.5)
      }
      for (const [, fill] of fillsFor(colors, colors.accent, colors.accentMuted)) {
        expect(contrast(parse(text['accent-on-tint']), fill)).toBeGreaterThanOrEqual(4.5)
      }
    },
  )

  it.each(themes)('%s: the label on the accent reaches 4.5:1 on the accent and on its hover fill', (_id, theme) => {
    const { colors, onAccent } = resolveTheme(theme)
    expect(contrast(parse(onAccent), parse(colors.accent))).toBeGreaterThanOrEqual(4.5)
    expect(contrast(parse(onAccent), parse(colors.accentHover))).toBeGreaterThanOrEqual(4.5)
  })

  it.each([...TEXT_COLOURS.map(([name]) => name), 'status-merged' as const])('--color-%s-text in the stylesheet defaults is the base light and dark value', (name) => {
    const values = [...css.matchAll(new RegExp(`--color-${name}-text: (#[0-9a-f]{6});`, 'gi'))].map((m) => m[1].toLowerCase())
    expect(values).toEqual([resolveTheme(lightTheme).text[name], resolveTheme(darkTheme).text[name]])
  })

  // Merged has no theme colour, so every theme derives its text version
  // from the base one against its own backgrounds.
  it.each(themes)('%s: text-status-merged reaches 4.5:1 on every background and its tint', (_id, theme) => {
    const { colors, text } = resolveTheme(theme)
    const merged = theme.meta.isDark ? MERGED.dark : MERGED.light
    for (const [, fill] of fillsFor(colors, merged.fill, merged.tint)) {
      expect(contrast(parse(text['status-merged']), fill)).toBeGreaterThanOrEqual(4.5)
    }
  })

  it('the stylesheet carries the base merged colours the themes derive from', () => {
    const fills = [...css.matchAll(/--color-status-merged: (#[0-9a-f]{6});/gi)].map((m) => m[1].toLowerCase())
    expect(fills).toEqual([MERGED.light.fill, MERGED.light.fill, MERGED.dark.fill].map((c) => c.toLowerCase()))
  })
})

// Non-text contrast (WCAG 1.4.11): what marks focus or state, and the
// edge of a text field, at 3:1 against what is around it.
describe('focus, state and field edges', () => {
  it.each(themes)('%s: the accent as a ring, outline, border or icon reaches 3:1 on every background and accent tint', (_id, theme) => {
    const { colors, accentUi } = resolveTheme(theme)
    const plain = Object.values(backgrounds({ colors } as Theme))
    const accent = parse(colors.accent)
    const tints = plain.flatMap((under) => [parse(colors.accentMuted, under), ...[0.1, 0.15, 0.2].map((a) => accent.map((v, i) => v * a + under[i] * (1 - a)) as Rgb)])
    for (const fill of [...plain, ...tints]) {
      expect(contrast(parse(accentUi), fill)).toBeGreaterThanOrEqual(3)
    }
  })

  it.each(themes)('%s: a text field\'s resting border reaches 3:1 on the app, the surface and the alternate surface', (_id, theme) => {
    const { colors, fieldBorder } = resolveTheme(theme)
    const { app, surface, surfaceAlt } = backgrounds({ colors } as Theme)
    for (const fill of [app, surface, surfaceAlt]) {
      expect(contrast(parse(fieldBorder), fill)).toBeGreaterThanOrEqual(3)
    }
  })

  it.each(['#FFD400', '#00C951', '#2C80FF', '#FF66B3'])('a workspace accent %s gets a ring colour at 3:1 in every theme', (accent) => {
    for (const [, theme] of themes) {
      const { colors, accentUi } = resolveTheme(theme, accent)
      for (const fill of Object.values(backgrounds({ colors } as Theme))) {
        expect(contrast(parse(accentUi), fill)).toBeGreaterThanOrEqual(3)
      }
    }
  })

  it.each(['accent-ui', 'field-border'] as const)('--color-%s in the stylesheet defaults is the base light and dark value', (name) => {
    const key = name === 'accent-ui' ? 'accentUi' : 'fieldBorder'
    const values = [...css.matchAll(new RegExp(`--color-${name}: (#[0-9a-f]{6});`, 'gi'))].map((m) => m[1].toLowerCase())
    expect(values).toEqual([resolveTheme(lightTheme)[key], resolveTheme(darkTheme)[key]])
  })
})
