import { afterEach, describe, expect, it, vi } from 'vitest'
import type { RouteLocationNormalized } from 'vue-router'

// The documentation page route loads the page before the view renders, and
// starts the editor's body (its saved copy) alongside the page request, so
// the body paints with the title.

const PAGE_UUID = '6f1c2b8e-4d3a-4f5b-9c7d-2e1a0b9c8d7f'

const docs = vi.hoisted(() => ({ getPageByPath: vi.fn(), getArticleById: vi.fn() }))
vi.mock('@nosdesk/core/services/documentationService', () => docs)
const prefetch = vi.hoisted(() => ({
  known: new Map<string, string>(),
  prefetchPageBody: vi.fn(),
}))
vi.mock('@/utils/docBodyPrefetch', () => ({
  pageUuidForPath: (path: string) => prefetch.known.get(path) ?? null,
  prefetchPageBody: prefetch.prefetchPageBody,
}))

import { preloadDocumentationPage } from '../documentationPreload'

const PAGE = { id: 5, uuid: PAGE_UUID, title: 'Payroll runbook', slug: 'payroll-runbook', children: [] }

function route(path: string): RouteLocationNormalized {
  return { params: { path }, meta: {} } as unknown as RouteLocationNormalized
}

afterEach(() => {
  prefetch.known.clear()
  vi.clearAllMocks()
})

describe('opening a documentation page', () => {
  it('starts the body of a page the pool knows before the page request answers', async () => {
    prefetch.known.set('payroll-runbook', PAGE_UUID)
    let answer!: (page: typeof PAGE) => void
    docs.getPageByPath.mockReturnValue(new Promise((resolve) => (answer = resolve)))

    const to = route('payroll-runbook')
    const entered = preloadDocumentationPage(to)
    await vi.waitFor(() => expect(docs.getPageByPath).toHaveBeenCalledWith('payroll-runbook'))
    await vi.waitFor(() => expect(prefetch.prefetchPageBody).toHaveBeenCalledWith(PAGE_UUID))

    answer(PAGE)
    expect(await entered).toBeUndefined()
    expect(to.meta.preloadedDocument).toEqual(PAGE)
    expect(to.meta.title).toBe('Payroll runbook')
  })

  it('starts the body from the page request when the pool does not know the page', async () => {
    docs.getPageByPath.mockResolvedValue(PAGE)

    await preloadDocumentationPage(route('payroll-runbook'))
    await vi.waitFor(() => expect(prefetch.prefetchPageBody).toHaveBeenCalledWith(PAGE_UUID))
  })

  it('reads a missing page as not found', async () => {
    docs.getPageByPath.mockResolvedValue(null)

    expect(await preloadDocumentationPage(route('gone'))).toEqual({
      name: 'error',
      params: { code: '404' },
      replace: true,
    })
    expect(prefetch.prefetchPageBody).not.toHaveBeenCalled()
  })
})
