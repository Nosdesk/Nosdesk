import { test, expect } from '@playwright/test'
import {
  BOARD,
  PROJECT_BOARD,
  PROJECT_SPARSE,
  PROJECT_TIMELINE,
  Touch,
  gotoAndSettle,
  login,
  visibleTimelineCard,
} from './helpers'

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
    await gotoAndSettle(page, `/projects/${PROJECT_BOARD}`, '[data-card-id]')

    // Separated from the visibility check on purpose: without this, a pool that
    // had not hydrated yet reads as "the board opened on the wrong column",
    // which is how this failed in CI while the landing scroll was working.
    const cardsRendered = await page.locator('[data-card-id]').count()
    expect(cardsRendered, 'the project should have cards to land on').toBeGreaterThan(0)

    // Columns render in workflow order, so the board would otherwise open on
    // Triage / Backlog — routinely empty — with the tickets several swipes away.
    //
    // Intersection, not full containment: the board snaps to columns, so it can
    // legitimately come to rest between two with a card straddling the edge.
    // Anything genuinely off-screen still reads as zero, so this keeps catching
    // the case it exists for — the board opening on an empty column with the
    // work several swipes away.
    //
    // The drag spec shares this project and rearranges its columns, which is
    // deliberate and harmless here: this asserts that the board lands on a
    // populated column, whichever one that happens to be.
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

  test('holding and moving a planned timeline block reschedules it, then restores it', async ({ page, context }) => {
    await login(page)
    await gotoAndSettle(page, `/projects/${PROJECT_TIMELINE}/gantt`, '[data-timeline-card-id]')

    // Not a skip. `seed-contract.spec.ts` already guarantees this project has a
    // planned block, so an absence here is a real failure — and the old skip was
    // unreachable anyway, because the selector wait above it timed out first.
    const card = await visibleTimelineCard(page)
    expect(card, 'seed contract guarantees a planned timeline block').not.toBeNull()

    const selector = `[data-timeline-card-id="${card!.id}"]`
    const before = await page.locator(selector).boundingBox()
    expect(before).not.toBeNull()

    // The timeline's scale is 36px/day. Two days clears the touch slop and
    // makes a snapped change unmistakable without running into the viewport
    // edge, where the intentionally-enabled auto-scroll would be a different
    // behaviour under test.
    const delta = 72
    const move = async (fromY: number, by: number): Promise<void> => {
      const pushed = page.waitForRequest((request) =>
        request.url().endsWith('/api/sync/push')
          && request.method() === 'POST'
          && request.postData()?.includes(`\"model_id\":\"${card!.id}\"`) === true
          && request.postData()?.includes('start_date') === true
          && request.postData()?.includes('due_date') === true,
      )
      const touch = await Touch.create(context, page)
      await touch.start(card!.x, fromY)
      await page.waitForTimeout(500)
      for (let i = 1; i <= 8; i++) {
        await touch.move(card!.x, fromY + (by * i) / 8)
        await page.waitForTimeout(16)
      }
      await touch.end()
      await pushed
    }

    await move(card!.y, delta)
    await expect.poll(async () => (await page.locator(selector).boundingBox())?.y).toBeGreaterThan(before!.y + 40)

    const moved = await page.locator(selector).boundingBox()
    expect(moved).not.toBeNull()
    await move(Math.round(moved!.y + moved!.height / 2), -delta)
    await expect.poll(async () => (await page.locator(selector).boundingBox())?.y).toBeLessThan(before!.y + 8)
  })
})
