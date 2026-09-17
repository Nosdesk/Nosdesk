import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import PaginationControls from '@/components/common/PaginationControls.vue'

let wrapper: VueWrapper | null = null
beforeEach(() => {
  // Desktop layout: the md breakpoint matches.
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query.includes('min-width'),
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  }))
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const base = { totalItems: 250, pageSize: 25, pageSizeOptions: [25, 50, 0], isInfiniteMode: false }

describe('PaginationControls', () => {
  it('is a named nav with the current page marked and edges kept', async () => {
    wrapper = mountWithProviders(PaginationControls, { ...base, currentPage: 10, totalPages: 20 })
    await nextTick()
    const nav = wrapper.get('nav')
    expect(nav.attributes('aria-label')).toBe('pagination-controls-nav-aria')
    const pages = nav.findAll('[data-type="page"]')
    expect(pages.map((p) => p.text())).toEqual(['1', '8', '9', '10', '11', '12', '20'])
    expect(nav.findAll('[data-type="ellipsis"]')).toHaveLength(2)
    const current = nav.get('[aria-current="page"]')
    expect(current.text()).toBe('10')
    expect(current.attributes('data-selected')).toBe('true')
  })

  it('uses the backend page count rather than deriving it', async () => {
    wrapper = mountWithProviders(PaginationControls, { ...base, currentPage: 1, totalPages: 3, totalItems: 1000 })
    await nextTick()
    expect(wrapper.findAll('[data-type="page"]').map((p) => p.text())).toEqual(['1', '2', '3'])
  })

  it('emits the page from the list, prev and next, and disables the ends', async () => {
    const pages: number[] = []
    wrapper = mountWithProviders(PaginationControls, {
      ...base,
      currentPage: 1,
      totalPages: 4,
      'onUpdate:currentPage': (p: number) => pages.push(p),
    })
    await nextTick()
    const prev = wrapper.get('[aria-label="pagination-controls-previous"]')
    const next = wrapper.get('[aria-label="pagination-controls-next"]')
    expect(prev.attributes('disabled')).toBeDefined()
    expect(next.attributes('disabled')).toBeUndefined()
    await next.trigger('click')
    expect(pages).toEqual([2])
    await wrapper.findAll('[data-type="page"]')[3].trigger('click')
    expect(pages).toEqual([2, 4])
  })

  it('renders no navigation in infinite mode', async () => {
    wrapper = mountWithProviders(PaginationControls, { ...base, currentPage: 1, totalPages: 1, pageSize: 0, isInfiniteMode: true })
    await nextTick()
    expect(wrapper.find('nav').exists()).toBe(false)
    expect(wrapper.text()).toContain('pagination-controls-items')
  })
})
