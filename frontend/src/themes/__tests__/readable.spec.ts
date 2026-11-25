import { afterEach, describe, expect, it, vi } from 'vitest'
import { themeRegistry } from '@/themes'
import { lightTheme } from '@/themes/base/light'
import { isColour, parseColour, toHex } from '@/themes/utils/readable'
import { resolveTheme } from '@/themes/utils/cssInjector'

afterEach(() => vi.restoreAllMocks())

describe('parseColour', () => {
  it.each([
    ['#f60', '#ff6600'],
    ['#F60', '#ff6600'],
    ['#ff6600', '#ff6600'],
    ['#f60f', '#ff6600'],
    ['#ff6600ff', '#ff6600'],
    ['rgb(255, 102, 0)', '#ff6600'],
    ['rgba(255, 102, 0, 1)', '#ff6600'],
    ['rgb(255 102 0)', '#ff6600'],
    ['  #ff6600 ', '#ff6600'],
  ])('reads %s', (input, hex) => {
    expect(toHex(parseColour(input))).toBe(hex)
  })

  it.each([
    ['#f600', '#ffffff', '#ffffff'],
    ['#ff660000', '#000000', '#000000'],
    ['#ff660080', '#000000', '#803300'],
  ])('lays the alpha of %s over %s', (input, under, hex) => {
    expect(toHex(parseColour(input, parseColour(under)))).toBe(hex)
  })

  it.each(['', 'banana', '#ff', '#ff66000', 'rgb(1, 2)', 'rgb(a, b, c)', 'hsl(20 100% 50%)', 'url(x)'])(
    'refuses %j',
    (input) => {
      expect(() => parseColour(input)).toThrow()
      expect(isColour(input)).toBe(false)
    },
  )
})

describe('resolveTheme with a workspace accent', () => {
  it.each(['#f60', '#f60c', '#ff6600', '#ff6600cc', 'rgb(255, 102, 0)'])('takes %s', (accent) => {
    const resolved = resolveTheme(lightTheme, accent)
    expect(resolved.colors.accent).toMatch(/^#[0-9a-f]{6}$/)
    expect(resolved.text.accent).toMatch(/^#[0-9a-f]{6}$/)
  })

  it.each(['banana', '#ff', 'rgb(1, 2)', '  ', 'javascript:alert(1)'])(
    'keeps the theme accent, and warns, when the workspace accent is %j',
    (accent) => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
      const resolved = resolveTheme(lightTheme, accent)
      expect(resolved.colors.accent).toBe(lightTheme.colors.accent)
      expect(warn).toHaveBeenCalled()
    },
  )

  it('never throws, even for a theme with a colour it cannot read', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const broken = { ...lightTheme, colors: { ...lightTheme.colors, surface: 'not a colour' } }
    expect(() => resolveTheme(broken, '#f60')).not.toThrow()
    expect(warn).toHaveBeenCalled()
  })

  it.each([...themeRegistry.keys()])('resolves %s with a three-digit accent', (id) => {
    expect(() => resolveTheme(themeRegistry.get(id)!, '#09c')).not.toThrow()
  })
})
