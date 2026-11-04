import { describe, expect, it, vi } from 'vitest'

vi.mock('@/utils/shareUrl', () => ({
  shareableTicketUrl: () => 'https://app.test/tickets/7?a=1&b="2"',
}))

import { useTicketDrag } from '@/composables/useTicketDrag'

describe('dragging a ticket into another app', () => {
  it('escapes the title and address in the rich-text link', () => {
    const data = new Map<string, string>()
    const event = {
      currentTarget: null,
      dataTransfer: {
        setData: (type: string, value: string) => data.set(type, value),
        setDragImage: () => {},
        effectAllowed: '',
      },
    } as unknown as DragEvent
    const { handleDragStart, endDrag } = useTicketDrag()
    handleDragStart({ id: 7, number: 7, title: '<img src=x onerror=alert(1)> "quoted"' } as never, 'kanban', event)
    endDrag()
    const html = data.get('text/html') ?? ''
    expect(html).not.toContain('<img')
    expect(html).toContain('&lt;img src=x onerror=alert(1)&gt; &quot;quoted&quot;')
    expect(html).toContain('href="https://app.test/tickets/7?a=1&amp;b=&quot;2&quot;"')
  })
})
