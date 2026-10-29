import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import postcss from 'postcss'
import { describe, expect, it } from 'vitest'
import { scopeCss } from '../pdfjs'

const scope = '.pdfjs-scope'

describe('scopeCss', () => {
  it('turns the root into the scope and prefixes everything else', () => {
    expect(scopeCss(':root { --a: 1 } :root:dir(rtl) { --b: 2 }', scope)).toBe(
      '.pdfjs-scope { --a: 1 } .pdfjs-scope:dir(rtl) { --b: 2 }',
    )
    expect(scopeCss('.page, .dialog > p { color: red }', scope)).toBe(
      '.pdfjs-scope .page, .pdfjs-scope .dialog > p { color: red }',
    )
  })

  it('prefixes rules inside media queries, and leaves keyframes and nested rules', () => {
    const out = scopeCss(
      '@media (prefers-color-scheme: dark) { :root { --c: 0 } .x { a: b } }' +
        '@keyframes spin { from { a: b } to { c: d } }' +
        '.textLayer { & .highlight { a: b } span { c: d } }',
      scope,
    )
    expect(out).toContain('@media (prefers-color-scheme: dark) { .pdfjs-scope { --c: 0 } .pdfjs-scope .x { a: b } }')
    expect(out).toContain('@keyframes spin { from { a: b } to { c: d } }')
    expect(out).toContain('.pdfjs-scope .textLayer { & .highlight { a: b } span { c: d } }')
  })

  it("leaves no selector of pdf.js's own stylesheet outside the scope", () => {
    const file = createRequire(import.meta.url).resolve('pdfjs-dist/web/pdf_viewer.css')
    const root = postcss.parse(scopeCss(readFileSync(file, 'utf8'), scope))
    const unscoped: string[] = []
    // A rule is top level when no rule (and no keyframes block) encloses it.
    const topLevel = (rule: postcss.Rule) => {
      for (let p = rule.parent; p && p.type !== 'root'; p = p.parent) {
        if (p.type === 'rule' || /keyframes$/i.test((p as postcss.AtRule).name)) return false
      }
      return true
    }
    root.walkRules((rule) => {
      if (!topLevel(rule)) return
      for (const selector of rule.selectors) if (!selector.startsWith(scope)) unscoped.push(selector)
    })
    expect(unscoped).toEqual([])
  })
})
