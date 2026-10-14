import { describe, expect, it } from 'vitest'
import enUS from '../../../../i18n/locales/en-US/main.ftl?raw'

// Every literal key the app passes to `t(...)` or `$t(...)` exists in the
// en-US catalogue, the fallback for every locale. A missing key renders as
// the raw key name. Keys built at runtime (template strings, variables)
// aren't checked here.

const sources = import.meta.glob(
  ['../../**/*.{vue,ts}', '../../../../packages/*/src/**/*.{vue,ts}', '!**/*.spec.ts', '!**/__tests__/**'],
  { query: '?raw', import: 'default', eager: true },
) as Record<string, string>

const defined = new Set(Array.from(enUS.matchAll(/^([a-zA-Z][\w-]*) =/gm), (m) => m[1]))

/** Lines that are comments carry example calls, not real ones. */
function code(source: string): string {
  return source
    .split('\n')
    .filter((line) => !/^\s*(\*|\/\/|\/\*)/.test(line))
    .join('\n')
}

describe('translation keys', () => {
  it('uses only keys the en-US catalogue defines', () => {
    const missing: string[] = []
    for (const [path, source] of Object.entries(sources)) {
      for (const m of code(source).matchAll(/(?<![\w.])\$?t\(\s*['"]([a-z][a-z0-9]*(?:-[a-z0-9]+)+)['"]/g)) {
        if (!defined.has(m[1])) missing.push(`${m[1]} (${path.replace(/^(\.\.\/)+/, '')})`)
      }
    }
    expect(missing).toEqual([])
  })
})
