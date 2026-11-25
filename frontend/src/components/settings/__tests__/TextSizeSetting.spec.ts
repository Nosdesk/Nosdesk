import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, shallowRef } from 'vue'
import { mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'

const device = shallowRef<{ scale: number; bold: boolean } | null>({ scale: 1.1176, bold: false })
let tauri = true

vi.mock('@/platform/deviceText', () => ({ FOLLOW_DEVICE_TEXT: true, deviceText: device }))
vi.mock('@/platform/runtime', () => ({ isTauriRuntime: () => tauri }))

const root = document.documentElement

async function render() {
  vi.resetModules()
  const { initTextSize } = await import('@/platform/textSize')
  initTextSize()
  const { default: TextSizeSetting } = await import('../TextSizeSetting.vue')
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  return mount(TextSizeSetting, { global: { plugins: [createPinia(), createFluentVue({ bundles: [bundle] })] }, attachTo: document.body })
}

const value = (w: Awaited<ReturnType<typeof render>>) => w.find('[data-text-size-value]').text()
const status = (w: Awaited<ReturnType<typeof render>>) => w.find('[data-text-size-status]').text()
const switchOn = (w: Awaited<ReturnType<typeof render>>) => w.find('[role="switch"]').attributes('aria-checked')

beforeEach(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia ??= ((query: string) => ({
    matches: false,
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
  })) as unknown as typeof window.matchMedia
  localStorage.clear()
  root.style.removeProperty('--user-text-scale')
  device.value = { scale: 1.1176, bold: false }
  tauri = true
})

describe('TextSizeSetting in the app', () => {
  it('opens following the device, and says so', async () => {
    const w = await render()
    expect(value(w)).toBe('112%')
    expect(switchOn(w)).toBe('true')
    expect(status(w)).toBe('This iPhone is set to larger text, and Nosdesk follows it.')
    expect(w.find('[data-text-size-status]').attributes('aria-live')).toBe('polite')
  })

  it('mentions Bold Text only while it is on', async () => {
    device.value = { scale: 1.1176, bold: true }
    const w = await render()
    expect(status(w)).toBe('This iPhone is set to larger, bold text, and Nosdesk follows it.')
  })

  it('moving the slider takes the size over, and the switch hands it back', async () => {
    const w = await render()
    await w.find('[role="slider"]').trigger('keydown', { key: 'ArrowRight' })
    await nextTick()
    expect(value(w)).toBe('125%')
    expect(switchOn(w)).toBe('false')
    expect(root.style.getPropertyValue('--user-text-scale')).toBe('1.25')
    expect(status(w)).toBe('Your own size, kept on this device.')

    await w.find('[role="switch"]').trigger('click')
    await nextTick()
    expect(value(w)).toBe('112%')
    expect(switchOn(w)).toBe('true')
    expect(root.style.getPropertyValue('--user-text-scale')).toBe('')
  })
})

describe('TextSizeSetting in a browser', () => {
  it('has no device switch, and its slider describes itself by the sentence', async () => {
    tauri = false
    device.value = null
    const w = await render()
    expect(w.find('[role="switch"]').exists()).toBe(false)
    expect(status(w)).toBe('Nosdesk follows the text size set in this browser.')
    const slider = w.find('[role="slider"]')
    expect(slider.attributes('aria-describedby')).toBe(w.find('[data-text-size-status]').attributes('id'))
    expect(slider.attributes('aria-valuemax')).toBe('3')
  })
})
