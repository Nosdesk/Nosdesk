import { afterEach, describe, expect, it } from 'vitest'
import { EMAIL_ZOOM_FLOOR, clampScale, fitEmailZoom, replyFieldLines } from '@/utils/textScale'
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
  it('shrinks a wide email to fit, never past 12px for 14px type', () => {
    expect(fitEmailZoom(1, 1)).toBe(1)
    expect(fitEmailZoom(2, 1.6)).toBe(1.25)
    expect(fitEmailZoom(1, 1.5)).toBe(EMAIL_ZOOM_FLOOR)
    expect(14 * EMAIL_ZOOM_FLOOR).toBeCloseTo(12)
  })
})
