import { describe, expect, it } from 'vitest'
import { ref } from 'vue'
import { labelGap, useAxisFont } from '../axisFont'

describe('useAxisFont', () => {
  it('reads the label size from the container and grows the gutters with it', () => {
    const el = document.createElement('div')
    el.style.fontSize = '12px'
    document.body.append(el)
    const width = ref(300)
    const { axisFont, gutter } = useAxisFont(ref(el), { width, height: ref(200) })
    expect(axisFont.value).toBe(12)
    expect(gutter(32)).toBe(35)
    el.style.fontSize = '22px'
    width.value = 301
    expect(axisFont.value).toBe(22)
    expect(gutter(22)).toBe(44)
    el.remove()
  })

  it('falls back to the designed 11px before the container exists', () => {
    const { axisFont, gutter } = useAxisFont(ref(null), { width: ref(0), height: ref(0) })
    expect(axisFont.value).toBe(11)
    expect(gutter(32)).toBe(32)
  })
})

describe('labelGap', () => {
  it('keeps the designed 64px at the designed size', () => {
    expect(labelGap(['Jun 15', 'Jun 22'], 11)).toBe(64)
  })

  it('grows with the label size, so fewer labels show at larger text', () => {
    expect(labelGap(['Jun 15'], 22)).toBe(128)
    // A long label at a large size: its own width plus one size of air.
    expect(labelGap(['Wednesday 15'], 24)).toBeCloseTo(12 * 24 * 0.6 + 24)
  })
})
