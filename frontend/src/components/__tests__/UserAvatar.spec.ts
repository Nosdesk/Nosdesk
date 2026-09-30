import { afterEach, describe, expect, it, vi } from 'vitest'
import type { VueWrapper } from '@vue/test-utils'
import { configureAssetUrl } from '@nosdesk/core/transport'
import { mountWithProviders } from '@/test/mountWithProviders'
import UserAvatar from '@/components/UserAvatar.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  configureAssetUrl((path) => path)
  vi.unstubAllGlobals()
})

function mountAvatar(props: Record<string, unknown> = {}) {
  wrapper = mountWithProviders(UserAvatar, {
    fallbackName: 'Noah Bennett',
    fallbackAvatar: '/uploads/users/avatars/noah.webp',
    showName: false,
    clickable: false,
    ...props,
  })
  return wrapper
}

/** The initials circle, the one div inside the avatar. */
function initials(w: VueWrapper): HTMLElement {
  return w.get('.avatar-themed').element.querySelector('div') as HTMLElement
}

describe('UserAvatar', () => {
  it('resolves the photo URL for the runtime, as the iOS app needs', () => {
    configureAssetUrl((path) => `nosdesk-asset://localhost${path}`)
    const w = mountAvatar()
    expect(w.get('img').attributes('src')).toBe('nosdesk-asset://localhost/uploads/users/avatars/noah.webp')
  })

  it('keeps the initials as the first child, names the avatar on hover, and hides the initials from screen readers under a photo', () => {
    const w = mountAvatar()
    const themed = w.get('.avatar-themed').element
    expect(themed.getAttribute('title')).toBe('Noah Bennett')
    // The themes restyle `.avatar-themed > div:first-child` and filter any img
    // inside, so nothing may wrap the initials and the photo together. (The test
    // renderer stands `<Transition>` in as an element; a browser renders none.)
    const first = initials(w)
    const parent = first.parentElement as HTMLElement
    expect(parent === themed || (parent.tagName === 'TRANSITION-STUB' && parent.parentElement === themed)).toBe(true)
    expect(w.get('img').element.parentElement).toBe(themed)
    expect(first.textContent?.trim()).toBe('NB')
    expect(first.getAttribute('aria-hidden')).toBe('true')
    expect(w.get('img').attributes('alt')).toBe('Noah Bennett')
  })

  it('fades the photo in, then hides the initials so a transparent upload does not show them', async () => {
    const w = mountAvatar()
    const img = w.get('img')
    expect(img.classes()).toContain('opacity-0')
    expect(img.classes()).toContain('motion-reduce:transition-none')
    await img.trigger('load')
    expect(img.classes()).toContain('opacity-100')
    expect(initials(w).classList.contains('invisible')).toBe(false)
    await img.trigger('transitionend')
    expect(initials(w).classList.contains('invisible')).toBe(true)
  })

  it('shows the photo at once when motion is reduced', async () => {
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: query.includes('reduce') }))
    const w = mountAvatar()
    await w.get('img').trigger('load')
    expect(initials(w).classList.contains('invisible')).toBe(true)
  })

  it('falls back to readable initials when the photo fails, and when there is none', async () => {
    const w = mountAvatar()
    await w.get('img').trigger('error')
    expect(w.find('img').exists()).toBe(false)
    expect(initials(w).getAttribute('aria-hidden')).toBeNull()
    expect(initials(w).classList.contains('invisible')).toBe(false)

    wrapper?.unmount()
    const none = mountAvatar({ fallbackAvatar: null })
    expect(none.find('img').exists()).toBe(false)
    expect(initials(none).textContent?.trim()).toBe('NB')
    expect(initials(none).getAttribute('aria-hidden')).toBeNull()
  })
})
