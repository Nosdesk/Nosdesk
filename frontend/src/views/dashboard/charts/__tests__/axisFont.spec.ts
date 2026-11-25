import { describe, expect, it } from 'vitest'
import { ref } from 'vue'
import { useAxisFont } from '../axisFont'

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
