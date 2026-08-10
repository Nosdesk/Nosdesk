import { test, expect } from '@playwright/test'
import { BOARD, PROJECT_SPARSE, PROJECT_WITH_TICKETS, gotoAndSettle, login } from './helpers'

const PROJECT_ROUTES = (id: number): Array<[string, string]> => [
  ['projects list', '/projects'],
  ['board', `/projects/${id}`],
  ['gantt', `/projects/${id}/gantt`],
  ['cycles', `/projects/${id}/cycles`],
]

/**
 * Layout invariants for the project views on a phone.
 *
 * These assert the properties that stay true regardless of design changes,
 * rather than pinning pixel values that would make every visual tweak a test
 * failure.
 */
test.describe('project views on a phone', () => {
  test.skip(({ hasTouch }) => !hasTouch, 'phone layout')

  for (const [name, path] of PROJECT_ROUTES(PROJECT_SPARSE)) {
    test(`${name} does not overflow the viewport`, async ({ page }) => {
      await login(page)
      await gotoAndSettle(page, path)

      // The cardinal mobile bug: the page itself scrolling sideways. Content
      // that scrolls horizontally on purpose (a board, a timeline) lives in its
      // own container and does not widen the document.
      const overflow = await page.evaluate(
        () =>
          Math.max(document.documentElement.scrollWidth, document.body.scrollWidth) -
          window.innerWidth,
      )
      expect(overflow, `${name} should not scroll sideways`).toBeLessThanOrEqual(0)
    })
  }

  test('no control renders outside the viewport', async ({ page }) => {
    await login(page)
    await gotoAndSettle(page, '/projects')

    // Regression: `hidden lg:inline-flex` passed to a component merged with the
    // component root's own `inline-flex`, so a desktop-only density toggle
    // rendered on phones and its buttons hung off the right edge.
    const bleeding = await page.evaluate(() => {
      const vw = window.innerWidth
      const scrollers = new Set<Element>()
      document.querySelectorAll('*').forEach((el) => {
        if (
          /(auto|scroll)/.test(getComputedStyle(el).overflowX) &&
          el.scrollWidth > el.clientWidth + 1
        ) {
          scrollers.add(el)
        }
      })
      const inScroller = (el: Element): boolean => {
        for (let p = el.parentElement; p; p = p.parentElement) if (scrollers.has(p)) return true
        return false
      }
      const out: string[] = []
      document.querySelectorAll('button, a, input, select').forEach((el) => {
        const r = el.getBoundingClientRect()
        if (r.width === 0 || r.height === 0) return
        if (r.right <= vw + 1) return
        if (inScroller(el) || getComputedStyle(el).position === 'fixed') return
        out.push(`${el.tagName.toLowerCase()} "${(el.textContent ?? '').trim().slice(0, 20)}"`)
      })
      return out
    })
    expect(bleeding, 'controls hanging off the right edge').toEqual([])
  })

  test('the board opens on a column that has work in it', async ({ page }) => {
    await login(page)
    await gotoAndSettle(page, `/projects/${PROJECT_WITH_TICKETS}`, '[data-card-id]')

    // Separated from the visibility check on purpose: without this, a pool that
    // had not hydrated yet reads as "the board opened on the wrong column",
    // which is how this failed in CI while the landing scroll was working.
    const cardsRendered = await page.locator('[data-card-id]').count()
    expect(cardsRendered, 'the project should have cards to land on').toBeGreaterThan(0)

    // Columns render in workflow order, so the board would otherwise open on
    // Triage / Backlog — routinely empty — with the tickets several swipes away.
    //
    // Intersection, not full containment. `board-touch-drag.spec.ts` drives this
    // same project and moves cards between columns, so which column the landing
    // scroll targets varies by the time this runs; demanding a card sit ENTIRELY
    // inside the viewport failed whenever the board came to rest between two
    // columns, which is a snap position rather than a bug. Anything off-screen
    // still reads as zero, so this keeps catching the case it exists for: the
    // board opening on an empty column with the work several swipes away.
    const cardsOnScreen = await page.evaluate((sel) => {
      const board = document.querySelector(sel)
      if (!board) return -1
      return [...board.querySelectorAll('[data-card-id]')].filter((el) => {
        const r = el.getBoundingClientRect()
        return r.width > 0 && r.right > 0 && r.left < window.innerWidth
      }).length
    }, BOARD)
    expect(cardsOnScreen, 'at least one card should be visible on open').toBeGreaterThan(0)
  })

  test('the gantt keeps its empty state on screen', async ({ page }) => {
    await login(page)
    await gotoAndSettle(page, `/projects/${PROJECT_SPARSE}/gantt`)

    // Regression: the hint carried a 240px minimum against a ~190px visible
    // timeline, so it overflowed and clipped its own text mid-sentence — the
    // one thing telling a user what to do next.
    const hint = page.locator('p', { hasText: /scheduled|due date/i }).first()
    if ((await hint.count()) === 0) test.skip(true, 'project has scheduled work; no empty state')

    const box = await hint.boundingBox()
    expect(box).not.toBeNull()
    const vw = page.viewportSize()!.width
    expect(box!.x, 'hint starts on screen').toBeGreaterThanOrEqual(0)
    expect(box!.x + box!.width, 'hint ends on screen').toBeLessThanOrEqual(vw + 1)
  })
})
