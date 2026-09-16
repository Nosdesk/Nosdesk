import { describe, expect, it } from 'vitest'

import type { Notification } from '@nosdesk/core/services/notificationService'
import { belongsToFilter, transformPages, type NotificationPage } from './notifications'

const row = (id: number, is_read: boolean, notification_type = 'comment_added'): Notification => ({
  id,
  uuid: `u${id}`,
  notification_type,
  title: `n${id}`,
  body: null,
  entity_type: 'ticket',
  entity_id: 1,
  is_read,
  created_at: `2026-09-17T10:00:0${id}`,
})

const page = (items: Notification[], next: NotificationPage['next'] = null): NotificationPage => ({
  items,
  next,
})
const list = (...pages: NotificationPage[]) => ({ pages, pageParams: pages.map(() => null) })
const titles = (l: { pages: NotificationPage[] } | undefined) =>
  l?.pages.flatMap((p) => p.items.map((n) => n.id)) ?? []

describe('belongsToFilter', () => {
  it('keeps read rows out of Unread and non-mentions out of Mentions', () => {
    expect(belongsToFilter('all', row(1, true))).toBe(true)
    expect(belongsToFilter('unread', row(1, true))).toBe(false)
    expect(belongsToFilter('unread', row(1, false))).toBe(true)
    expect(belongsToFilter('mentions', row(1, false))).toBe(false)
    expect(belongsToFilter('mentions', row(1, false, 'mentioned'))).toBe(true)
  })
})

describe('transformPages', () => {
  it('marks a row read in every cache, drops it from Unread, and counts once', () => {
    const mention = row(2, false, 'mentioned')
    const lists = {
      all: list(page([row(1, false), mention, row(3, true)])),
      unread: list(page([row(1, false), mention])),
      mentions: list(page([mention])),
    }
    const { lists: out, delta } = transformPages(lists, (n) =>
      n.id === 2 ? { ...n, is_read: true } : n,
    )
    expect(delta).toBe(-1)
    expect(titles(out.all)).toEqual([1, 2, 3])
    expect(out.all?.pages[0].items[1].is_read).toBe(true)
    expect(titles(out.unread)).toEqual([1])
    expect(titles(out.mentions)).toEqual([2])
    expect(out.mentions?.pages[0].items[0].is_read).toBe(true)
  })

  it('removes rows from every cache and counts only the unread ones', () => {
    const lists = {
      all: list(page([row(1, false), row(2, true), row(3, false)])),
      unread: list(page([row(1, false), row(3, false)])),
    }
    const gone = new Set([1, 2])
    const { lists: out, delta } = transformPages(lists, (n) => (gone.has(n.id) ? null : n))
    expect(delta).toBe(-1)
    expect(titles(out.all)).toEqual([3])
    expect(titles(out.unread)).toEqual([3])
  })

  it('keeps the page cursor untouched so hasMore survives removals', () => {
    const next = { before: '2026-09-17T10:00:01', before_id: 1 }
    const lists = { unread: list(page([row(1, false)], next)) }
    const { lists: out } = transformPages(lists, () => null)
    expect(titles(out.unread)).toEqual([])
    expect(out.unread?.pages[0].next).toEqual(next)
  })

  it('skips caches that are not loaded', () => {
    const { lists: out, delta } = transformPages({ all: list(page([row(1, false)])) }, (n) => ({
      ...n,
      is_read: true,
    }))
    expect(Object.keys(out)).toEqual(['all'])
    expect(delta).toBe(-1)
  })
})
