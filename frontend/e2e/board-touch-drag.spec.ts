import { test, expect } from '@playwright/test'
import {
  BOARD,
  PROJECT_WITH_TICKETS,
  Touch,
  boardScrollLeft,
  columnOfCard,
  dragDirection,
  panDirection,
  gotoAndSettle,
  login,
  visibleCard,
} from './helpers'

/**
 * Kanban drag-and-drop on touch.
 *
 * This is the spec that matters most, because its failure mode is invisible:
 * the board renders perfectly while dragging a card silently pans it instead.
 * That shipped, and was only found by driving a real touch gesture.
 *
 * The rule is swipe-to-pan, hold-to-drag. Both halves are asserted, because
 * "fixing" the drag by making cards `touch-action: none` would pass a
 * drag-works test while destroying the ability to scroll the board at all.
 */
test.describe('kanban touch drag', () => {
  test.skip(({ hasTouch }) => !hasTouch, 'touch-only behaviour')

  test('a quick swipe on a card pans the board', async ({ page, context }) => {
    await login(page)
    await gotoAndSettle(page, `/projects/${PROJECT_WITH_TICKETS}`)

    const card = await visibleCard(page)
    expect(card, 'a card should be on screen; the board lands on a populated column').not.toBeNull()

    const before = await boardScrollLeft(page)
    const dx = await panDirection(page)
    const touch = await Touch.create(context, page)
    await touch.start(card!.x, card!.y)
    await touch.drag(card!.x, card!.y, dx, page)
    await touch.end()
    await page.waitForTimeout(500)

    expect(await boardScrollLeft(page), 'swiping a card must still scroll the board').not.toBe(
      before,
    )
  })

  test('holding a card picks it up, and the board stays put', async ({ page, context }) => {
    await login(page)
    await gotoAndSettle(page, `/projects/${PROJECT_WITH_TICKETS}`)

    const card = await visibleCard(page)
    expect(card).not.toBeNull()

    const before = await boardScrollLeft(page)
    const touch = await Touch.create(context, page)
    await touch.start(card!.x, card!.y)
    // Past the long-press threshold (350ms) without moving.
    await page.waitForTimeout(500)
    await touch.drag(card!.x, card!.y, 200, page, 14)

    const during = await boardScrollLeft(page)
    await touch.end()
    await page.waitForTimeout(1200)

    expect(during, 'once the hold has activated, the drag owns the gesture').toBe(before)
  })

  test('dropping on another column moves the card', async ({ page, context }) => {
    await login(page)
    await gotoAndSettle(page, `/projects/${PROJECT_WITH_TICKETS}`)

    const card = await visibleCard(page)
    expect(card).not.toBeNull()
    const from = await columnOfCard(page, card!.id)
    expect(from, 'card should start in a resolvable column').not.toBeNull()

    const dir = await dragDirection(page, card!.id)
    const touch = await Touch.create(context, page)
    await touch.start(card!.x, card!.y)
    await page.waitForTimeout(500)
    // Onto a neighbouring column that actually exists in that direction.
    await touch.drag(card!.x, card!.y, 200 * dir, page, 14)
    await touch.end()
    await page.waitForTimeout(1500)

    // Assert it MOVED rather than landing anywhere specific: these specs run
    // against shared demo data, so the starting column varies between runs.
    const after = await columnOfCard(page, card!.id)
    expect(after, `#${card!.id} should have left "${from}"`).not.toBe(from)
  })
})

test.describe('kanban mouse drag', () => {
  test.skip(({ hasTouch }) => hasTouch, 'pointer-device behaviour')

  /** The touch work refactored the shared activation path, so the mouse path
   *  is asserted too: it must still promote on distance, with no hold. */
  test('dragging with a mouse still moves a card', async ({ page }) => {
    await login(page)
    await gotoAndSettle(page, `/projects/${PROJECT_WITH_TICKETS}`)

    const card = await visibleCard(page)
    expect(card).not.toBeNull()
    const from = await columnOfCard(page, card!.id)
    expect(from).not.toBeNull()

    const dir = await dragDirection(page, card!.id)
    await page.mouse.move(card!.x, card!.y)
    await page.mouse.down()
    for (let i = 1; i <= 12; i++) {
      await page.mouse.move(card!.x + i * 28 * dir, card!.y)
      await page.waitForTimeout(16)
    }
    await page.mouse.up()
    await page.waitForTimeout(1500)

    const after = await columnOfCard(page, card!.id)
    expect(after, `#${card!.id} should have left "${from}"`).not.toBe(from)
    await expect(page.locator(BOARD)).toBeVisible()
  })
})
