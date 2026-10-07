import { afterEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { configureAssetUrl } from '@nosdesk/core/transport'
import AssetImg from '@/components/common/AssetImg.vue'

afterEach(() => configureAssetUrl((p) => p, (u) => u))

describe('AssetImg', () => {
  it('loads a stored path through the platform resolver', () => {
    configureAssetUrl((p) => (p.startsWith('/') ? `nosdesk-asset://localhost${p}` : p))
    const img = mount(AssetImg, { props: { src: '/uploads/branding/logo.png' } }).find('img')
    expect(img.attributes('src')).toBe('nosdesk-asset://localhost/uploads/branding/logo.png')
  })

  it('passes everything else to the image', async () => {
    const onError = vi.fn()
    const wrapper = mount(AssetImg, {
      props: { src: 'data:image/png;base64,AAAA' },
      attrs: { alt: 'Logo', class: 'h-8', onError },
    })
    const img = wrapper.find('img')
    expect(img.attributes()).toMatchObject({ alt: 'Logo', class: 'h-8', src: 'data:image/png;base64,AAAA' })
    await img.trigger('error')
    expect(onError).toHaveBeenCalled()
  })

  it('renders no src without one', () => {
    expect(mount(AssetImg, { props: { src: null } }).find('img').attributes('src')).toBeUndefined()
  })
})
