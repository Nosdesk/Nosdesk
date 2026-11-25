import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

// The type scale lives in main.css; these hold the parts of it that are
// easy to break by adding a size in one place and not the others.
const css = readFileSync(resolve(__dirname, '../main.css'), 'utf8')

/** The declarations of the first rule whose header matches. */
function block(header: RegExp): Map<string, string> {
  const at = css.search(header)
  if (at === -1) throw new Error(`no rule matching ${header}`)
  const open = css.indexOf('{', at)
  let depth = 0
  let end = open
  for (; end < css.length; end++) {
    if (css[end] === '{') depth++
    if (css[end] === '}' && --depth === 0) break
  }
  const body = css.slice(open + 1, end).replace(/\/\*[\s\S]*?\*\//g, '')
  return new Map([...body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]))
}

const theme = block(/^@theme \{/m)
const sizes = [...theme].filter(([name]) => /^--text-[\w-]+$/.test(name) && !name.includes('--', 2))

/** The rem a body-relative size works out to at scale 1. */
function remAtOne(value: string): number {
  const scaled = /^calc\(([\d.]+)rem \* var\(--text-scale\)(?: \+ ([\d.]+)rem)?\)$/.exec(value)
  if (!scaled) throw new Error(`not a scaled size: ${value}`)
  return Number(scaled[1]) + Number(scaled[2] ?? 0)
}

const small = sizes.filter(([, value]) => remAtOne(value) < 1).map(([name]) => name)

describe('the type scale', () => {
  it('composes the person\'s size over the device\'s, held to 1 to 2', () => {
    expect(block(/^:root \{\n {2}--text-scale/m).get('--text-scale')).toBe(
      'clamp(1, var(--user-text-scale, var(--device-text-scale, 1)), 2)',
    )
  })

  it('multiplies every size by --text-scale', () => {
    expect(sizes.length).toBeGreaterThan(10)
    for (const [, value] of sizes) expect(() => remAtOne(value)).not.toThrow()
  })

  it('keeps headings and numerals a fixed lead over body text, so they never fall under it', () => {
    for (const [name, value] of sizes) {
      if (remAtOne(value) <= 1) continue
      expect(value, name).toMatch(/^calc\(1rem \* var\(--text-scale\) \+ [\d.]+rem\)$/)
    }
  })

  it('lifts every size under 12px to 12px on a phone, and moves nothing down', () => {
    const phone = block(/^@media screen and \(width < 48rem\) \{/m)
    for (const name of small) {
      const lifted = phone.get(name)
      const atOne = lifted ? remAtOne(lifted) : remAtOne(theme.get(name)!)
      expect(atOne, name).toBeGreaterThanOrEqual(0.75)
      expect(atOne, name).toBeGreaterThanOrEqual(remAtOne(theme.get(name)!))
    }
  })

  it('raises every size under body size to body size in an input, textarea or select on touch', () => {
    const fields = block(/^ {4}textarea,\n {4}select \{/m)
    for (const name of small) expect(fields.get(name), name).toBe('var(--text-base)')
  })

  it('pins every size inside .type-fixed to its unscaled value', () => {
    const fixed = block(/^\.type-fixed \{/m)
    for (const [name, value] of sizes) expect(fixed.get(name), name).toBe(`${remAtOne(value)}rem`)
  })
})
