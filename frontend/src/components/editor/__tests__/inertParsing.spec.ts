import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'

// The share URL helper imports the router, and with it every view.
vi.mock('@/utils/shareUrl', () => ({ shareableTicketUrl: () => '/tickets/7' }))
// The ticket link's card waits on a fetch that never answers.
vi.mock('@nosdesk/core/services/ticketService', () => ({
  default: {},
  getTicketById: () => new Promise(() => {}),
}))

import { mountWithProviders } from '@/test/mountWithProviders'
import SimpleEditor from '@/components/common/SimpleEditor.vue'
import { referencedTicketIds } from '@/components/editor/ticketLinkPlugin'
import { htmlHasText } from '@nosdesk/core/utils/inertHtml'

// Composer HTML can carry text a requester wrote (a saved reply fills in the
// ticket's title and the customer's name). Reading it must not build it in
// the page's own document, where its elements would start loading.

const MARKER = 'src="x"'
const HTML = `<p>See <span data-ticket-link data-ticket-id="7">#7</span><iframe ${MARKER}></iframe></p>`

/** Markup assigned through `innerHTML` to an element of the live document. */
const liveWrites: string[] = []
const innerHTML = Object.getOwnPropertyDescriptor(Element.prototype, 'innerHTML')!

beforeEach(() => {
  liveWrites.length = 0
  Object.defineProperty(Element.prototype, 'innerHTML', {
    ...innerHTML,
    set(this: Element, value: string) {
      if (this.ownerDocument === document) liveWrites.push(String(value))
      innerHTML.set!.call(this, value)
    },
  })
})
afterEach(() => {
  Object.defineProperty(Element.prototype, 'innerHTML', innerHTML)
  document.body.innerHTML = ''
})

describe('composer HTML is parsed off the page', () => {
  it('finds the referenced tickets', () => {
    expect(referencedTicketIds(HTML)).toEqual([7])
    expect(liveWrites).toEqual([])
  })

  it('checks the composer has text', () => {
    expect(htmlHasText(HTML)).toBe(true)
    expect(htmlHasText(`<p> </p><iframe ${MARKER}></iframe>`)).toBe(false)
    expect(liveWrites).toEqual([])
  })

  it('loads the editor value', async () => {
    const wrapper = mountWithProviders(SimpleEditor, { modelValue: HTML })
    await flushPromises()
    await wrapper.setProps({ modelValue: `${HTML}<p>more</p>` })
    await flushPromises()
    expect(wrapper.text()).toContain('more')
    expect(liveWrites.filter((w) => w.includes(MARKER))).toEqual([])
    wrapper.unmount()
  })
})
