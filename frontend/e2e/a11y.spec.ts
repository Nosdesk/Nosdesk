import { test, expect, type Page } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'
import { gotoAndSettle } from './helpers'

/**
 * Accessibility floor for the surfaces being moved onto Reka UI. Each
 * migration PR adds its surface here, so the floor only rises.
 *
 * axe runs with the WCAG 2.x A/AA tags and fails on serious or critical
 * impact. Rules listed in BASELINE are findings that predate this floor
 * and have an owner in the Reka sequence; each entry is removed by the PR
 * that fixes it, so the list can only shrink (a new rule id here is a
 * review question, the same ratchet as scripts/button-baseline.json).
 */
const BASELINE: Record<string, string> = {
  // DataTable puts aria-sort on the header's inner button, not the <th>.
  // Fixed with the table header rework.
  'aria-allowed-attr': 'DataTable sortable headers',
  // Tertiary text at text-2xs on tinted rows. Needs the token review, not a
  // component swap.
  'color-contrast': 'tertiary text scale',
  // Inbox rows are a button that contains action buttons. Fixed when the
  // inbox row moves onto the menu recipe (PR 4).
  'nested-interactive': 'NotificationInboxView rows',
}

async function expectNoSeriousViolations(page: Page, scope?: string): Promise<void> {
  let builder = new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .disableRules(Object.keys(BASELINE))
  if (scope) builder = builder.include(scope)
  const results = await builder.analyze()
  const serious = results.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical')
  expect(
    serious.map((v) => `${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(' ')).join(', ')})`),
  ).toEqual([])
}

test.describe('accessibility floor', () => {
  test.skip(({ hasTouch }) => hasTouch, 'hover and focus are pointer/keyboard concerns')

  test('icon buttons expose a tooltip on hover and focus and describe the trigger', async ({ page }) => {
    // The ticket detail sidebar always renders icon buttons (clear
    // requester/assignee, scheduling); the list views only do so
    // conditionally.
    await gotoAndSettle(page, '/tickets')
    // Desktop rows open on double-click when the split preview is on and
    // on click otherwise; a double-click covers both.
    await page.locator('table tbody tr').first().dblclick()
    await page.waitForURL(/\/tickets\/\d+/)
    // Let the page's own autofocus (the editor) settle before we move focus.
    await page.waitForTimeout(3000)

    const trigger = page.locator('button[data-icon-only]').first()
    await expect(trigger).toBeAttached()
    const name = (await trigger.getAttribute('aria-label')) ?? ''
    expect(name).not.toBe('')
    // Reka renders the visible bubble plus a visually-hidden role=tooltip
    // node that carries the accessible text; the bubble is what a sighted
    // user sees, the role node is what a screen reader reads.
    const bubble = page.locator('#overlays [data-state="delayed-open"], #overlays [data-state="instant-open"]')
    const tooltip = page.locator('#overlays [role="tooltip"]')

    await trigger.hover()
    await expect(bubble.first()).toBeVisible()
    await expect(tooltip).toHaveText(name)
    await expect(trigger).toHaveAttribute('aria-describedby', /.+/)

    await page.keyboard.press('Escape')
    await expect(tooltip).toHaveCount(0)

    await page.mouse.move(0, 0)
    await trigger.focus()
    await expect(tooltip).toHaveText(name)
  })

  test('a dialog traps focus, hides the page and restores focus on close', async ({ page }) => {
    // Dashboard > Edit dashboard > Add widget opens a Modal with tabs and
    // a list, enough to exercise the trap.
    await gotoAndSettle(page, '/')
    const edit = page.getByRole('button', { name: 'Edit dashboard' })
    await edit.click()
    const opener = page.getByRole('button', { name: 'Add widget' })
    await opener.click()

    const dialog = page.getByRole('dialog')
    await expect(dialog).toBeVisible()
    await expect(dialog).toHaveAccessibleName('Add widget')
    // Everything outside the dialog is hidden from assistive tech. The
    // aria-hidden library keeps live regions reachable, and the app has
    // three inside #app, so the marks land on the page chrome rather than
    // on #app itself; the sidebar nav is a representative sibling.
    const nav = page.locator('nav').first()
    await expect(nav).toHaveAttribute('aria-hidden', 'true')
    // Focus starts inside and Tab never leaves.
    await expect.poll(() => page.evaluate(() => document.activeElement?.closest('[role="dialog"]') !== null)).toBe(true)
    for (let i = 0; i < 12; i++) await page.keyboard.press('Tab')
    expect(await page.evaluate(() => document.activeElement?.closest('[role="dialog"]') !== null)).toBe(true)
    await expectNoSeriousViolations(page, '[role="dialog"]')

    await page.keyboard.press('Escape')
    await expect(dialog).toHaveCount(0)
    await expect(nav).not.toHaveAttribute('aria-hidden', 'true')
    // Focus returns to the opener.
    await expect(opener).toBeFocused()
  })

  test('tickets list has no serious violations', async ({ page }) => {
    await gotoAndSettle(page, '/tickets')
    await expectNoSeriousViolations(page)
  })

  test('inbox has no serious violations', async ({ page }) => {
    await gotoAndSettle(page, '/inbox')
    await expectNoSeriousViolations(page)
  })
})
