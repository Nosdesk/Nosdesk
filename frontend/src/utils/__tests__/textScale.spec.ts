import { afterEach, describe, expect, it } from 'vitest'
import { clampScale, fitEmailZoom, replyFieldLines } from '@/utils/textScale'
import { readTextScale } from '@/composables/useTextScale'

describe('clampScale', () => {
  it('holds the scale to 1 to 2', () => {
    expect(clampScale(0.82)).toBe(1)
    expect(clampScale(1.1176)).toBe(1.1176)
    expect(clampScale(3.1)).toBe(2)
    expect(clampScale(Number.NaN)).toBe(1)
  })
})

describe('replyFieldLines', () => {
  it('grows to five lines, fewer once five would pass 150px, never under two', () => {
    expect(replyFieldLines(1)).toBe(5)
    expect(replyFieldLines(1.1176)).toBe(5)
    expect(replyFieldLines(1.25)).toBe(5)
    expect(replyFieldLines(1.5)).toBe(4)
    expect(replyFieldLines(1.75)).toBe(3)
    expect(replyFieldLines(2)).toBe(3)
  })
})

describe('readTextScale', () => {
  const root = document.documentElement
  afterEach(() => {
    root.style.removeProperty('--user-text-scale')
    root.style.removeProperty('--device-text-scale')
  })

  it('falls back to the inputs where the property is not registered', () => {
    expect(readTextScale()).toBe(1)
    root.style.setProperty('--device-text-scale', '1.1176')
    expect(readTextScale()).toBe(1.1176)
    root.style.setProperty('--user-text-scale', '1.5')
    expect(readTextScale()).toBe(1.5)
    root.style.setProperty('--user-text-scale', '4')
    expect(readTextScale()).toBe(2)
  })
})

describe('fitEmailZoom', () => {
  it('shrinks a wide email to fit while its body text stays at 12px or more', () => {
    expect(fitEmailZoom(1, 1, 14)).toBe(1)
    // 14px body at 200%: fits at 1.25, and 14 * 1.25 is over 12.
    expect(fitEmailZoom(2, 1.6, 14)).toBe(1.25)
    // 14px body at 100%: the fit (0.67) would show it at 9.3px; held at 12/14.
    expect(fitEmailZoom(1, 1.5, 14)).toBeCloseTo(12 / 14)
    // A 16px body may come down to 12px, and its 10px footer with it.
    expect(16 * fitEmailZoom(1, 4, 16)).toBeCloseTo(12)
  })

  it('never shrinks body text the sender already set under 12px', () => {
    expect(fitEmailZoom(1, 2, 11)).toBe(1)
    // An 11px body at 150% shows at 16.5px: it may come down to 12px.
    expect(fitEmailZoom(1.5, 3, 11)).toBeCloseTo(12 / 11)
  })
})
