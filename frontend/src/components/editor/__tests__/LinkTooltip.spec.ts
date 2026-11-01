import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'
import LinkTooltip from '../LinkTooltip.vue'

// The editor mounts the tooltip hidden and shows it when a link is added.
async function mountTooltip() {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  const w = mount(LinkTooltip, {
    props: { visible: false, url: '', x: 0, y: 0, isEditing: true },
    global: { plugins: [createFluentVue({ bundles: [bundle] })] },
    attachTo: document.body,
  })
  await w.setProps({ visible: true })
  return w
}

describe('link dialog', () => {
  it('refuses a script address and says why, until it is changed', async () => {
    const w = await mountTooltip()
    const input = w.find('input')
    await input.setValue('javascript:alert(document.domain)')
    await input.trigger('keydown', { key: 'Enter' })
    expect(w.emitted('apply')).toBeUndefined()
    expect(w.text()).toContain('Links can go to a web, email or phone address.')
    expect(input.attributes('aria-invalid')).toBe('true')

    await input.setValue('example.com')
    expect(w.text()).not.toContain('Links can go to')
    await input.trigger('keydown', { key: 'Enter' })
    expect(w.emitted('apply')).toEqual([['https://example.com']])
    w.unmount()
  })

  it('keeps a mail address as typed', async () => {
    const w = await mountTooltip()
    const input = w.find('input')
    await input.setValue('mailto:help@example.com')
    await input.trigger('keydown', { key: 'Enter' })
    expect(w.emitted('apply')).toEqual([['mailto:help@example.com']])
    w.unmount()
  })
})
