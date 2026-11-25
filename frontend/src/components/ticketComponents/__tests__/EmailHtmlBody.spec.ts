import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'
import EmailHtmlBody from '../EmailHtmlBody.vue'

/**
 * jsdom has no layout, so the frame's document is a stand-in whose width
 * follows the zoom the component sets: `natural` CSS px wide as sent, and
 * `reflowed` once the reflow step caps the sender's widths.
 */
function fakeFrame(
  frame: HTMLIFrameElement,
  { natural, reflowed, textPx = 14, html }: { natural: number; reflowed: number; textPx?: number; html?: string },
) {
  const doc = document.implementation.createHTMLDocument('')
  doc.body.innerHTML = html ?? `<p style="font-size:${textPx}px">Your service call is booked.</p>`
  const zoom = () => Number(doc.body.style.zoom || 1)
  const width = () => ('reflow' in doc.body.dataset ? reflowed : natural) * zoom()
  Object.defineProperty(frame, 'contentDocument', { value: doc })
  Object.defineProperty(frame, 'clientWidth', { value: 300 })
  Object.defineProperty(doc.documentElement, 'clientWidth', { value: 300 })
  Object.defineProperty(doc.documentElement, 'scrollWidth', { get: () => Math.max(300, width()) })
  Object.defineProperty(doc, 'defaultView', { value: window })
  for (const el of doc.body.querySelectorAll('*')) {
    ;(el as HTMLElement).getClientRects = () => [{}] as unknown as DOMRectList
  }
  return doc
}

function mountEmail(props: Record<string, unknown> = {}) {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  return mount(EmailHtmlBody, {
    props: { html: '<p>x</p>', preSanitised: true, ...props },
    global: { plugins: [createFluentVue({ bundles: [bundle] })] },
  })
}

async function load(wrapper: ReturnType<typeof mountEmail>, sizes: Parameters<typeof fakeFrame>[1]) {
  const frame = wrapper.find('iframe').element as HTMLIFrameElement
  const doc = fakeFrame(frame, sizes)
  frame.dispatchEvent(new Event('load'))
  await nextTick()
  return doc
}

const root = document.documentElement

beforeEach(() => {
  vi.stubGlobal('CSS', { supports: () => true })
})
afterEach(() => {
  vi.unstubAllGlobals()
  root.style.removeProperty('--user-text-scale')
})

describe('EmailHtmlBody', () => {
  it('leaves an email that fits as sent, with no size switch', async () => {
    const wrapper = mountEmail()
    const doc = await load(wrapper, { natural: 280, reflowed: 280 })
    expect(doc.body.style.zoom).toBe('')
    expect('reflow' in doc.body.dataset).toBe(false)
    expect(wrapper.find('[data-email-size]').exists()).toBe(false)
  })

  it('reflows a wide email before it shrinks it', async () => {
    const wrapper = mountEmail()
    const doc = await load(wrapper, { natural: 600, reflowed: 290 })
    expect('reflow' in doc.body.dataset).toBe(true)
    expect(doc.body.style.zoom).toBe('')
    expect(wrapper.find('[data-email-size]').exists()).toBe(false)
  })

  it('shrinks what cannot wrap, offers its own switch, and stops where 14px text shows at 12px', async () => {
    const wrapper = mountEmail()
    const doc = await load(wrapper, { natural: 900, reflowed: 600 })
    expect(Number(doc.body.style.zoom)).toBeCloseTo(12 / 14)
    expect(doc.documentElement.style.overflowX).toBe('auto')
    const toggle = wrapper.find('[data-email-size]')
    expect(toggle.exists()).toBe(true)
    expect(toggle.text()).toBe('Show at my text size')
    expect(wrapper.emitted('fitted')?.at(-1)).toEqual([true])

    await toggle.trigger('click')
    await nextTick()
    expect(doc.body.style.zoom).toBe('')
    expect(wrapper.find('[data-email-size]').text()).toBe('Fit to width')
  })

  it('shrinks only as far as it must at the reader text size', async () => {
    root.style.setProperty('--user-text-scale', '2')
    await nextTick()
    const wrapper = mountEmail()
    const doc = await load(wrapper, { natural: 900, reflowed: 200 })
    // 200px at 2x is 400 in a 300px frame: 1.5 fits, and 14px shows at 21px.
    expect(Number(doc.body.style.zoom)).toBeCloseTo(1.5)
    expect(doc.documentElement.style.overflowX).toBe('hidden')
  })

  it('leaves the switch to a parent that owns the size', async () => {
    const wrapper = mountEmail({ size: 'fit' })
    await load(wrapper, { natural: 900, reflowed: 600 })
    expect(wrapper.find('[data-email-size]').exists()).toBe(false)
    expect(wrapper.emitted('fitted')?.at(-1)).toEqual([true])
  })

  it('pans with no switch when the body text is already at the floor', async () => {
    const wrapper = mountEmail()
    const doc = await load(wrapper, { natural: 900, reflowed: 600, textPx: 11 })
    expect(doc.body.style.zoom).toBe('')
    expect(doc.documentElement.style.overflowX).toBe('auto')
    expect(wrapper.find('[data-email-size]').exists()).toBe(false)
    expect(wrapper.emitted('fitted')).toBeUndefined()
  })

  it('frees only declared widths wider than the frame, never an image', async () => {
    const wrapper = mountEmail()
    const doc = await load(wrapper, {
      natural: 600,
      reflowed: 290,
      html: `<table width="600"><tr><td style="width:552px"><img src="x.png" style="width:640px"><div style="width:40px"></div><p style="font-size:14px">Hello</p></td></tr></table><div style="width:60%"></div>`,
    })
    const marked = [...doc.querySelectorAll('[data-reflow-width]')].map((el) => el.tagName)
    expect(marked).toEqual(['TABLE', 'TD'])
  })
})
