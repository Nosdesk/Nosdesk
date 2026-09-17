import { test, expect, type Page } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'
import { PROJECT_TIMELINE, gotoAndSettle } from './helpers'

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

  test('a popover opens on its trigger, toggles, and closes on Escape', async ({ page }) => {
    await gotoAndSettle(page, '/tickets')
    const trigger = page.getByRole('button', { name: 'Display' })
    await trigger.click()
    const menu = page.locator('#overlays [role="dialog"]')
    await expect(menu).toBeVisible()
    await expect(trigger).toHaveAttribute('aria-expanded', 'true')
    await expectNoSeriousViolations(page, '#overlays')
    // Clicking the trigger again toggles closed rather than close-then-reopen.
    await trigger.click()
    await expect(menu).toHaveCount(0)
    await trigger.click()
    await expect(menu).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(menu).toHaveCount(0)
    await expect(trigger).toHaveAttribute('aria-expanded', 'false')
  })

  test('an action menu walks rows with arrow keys and restores focus on Escape', async ({ page }) => {
    await gotoAndSettle(page, '/tickets')
    const trigger = page.getByRole('button', { name: 'User menu' })
    await trigger.click()
    const menu = page.locator('#overlays [role="menu"]')
    await expect(menu).toBeVisible()
    await expectNoSeriousViolations(page, '#overlays')
    const items = menu.getByRole('menuitem')
    expect(await items.count()).toBeGreaterThan(1)
    // ArrowDown lands on the first row, then walks; typeahead is Reka's.
    await page.keyboard.press('ArrowDown')
    await expect(items.first()).toBeFocused()
    await page.keyboard.press('ArrowDown')
    await expect(items.nth(1)).toBeFocused()
    await page.keyboard.press('End')
    await expect(items.last()).toBeFocused()
    await page.keyboard.press('Escape')
    await expect(menu).toHaveCount(0)
    await expect(trigger).toBeFocused()
  })

  test('a select opens a listbox, walks options with arrow keys, and closes on Escape', async ({ page }) => {
    await gotoAndSettle(page, `/projects/${PROJECT_TIMELINE}/gantt`)
    // A CSS locator rather than getByRole: Select is modal and hides
    // everything outside the open listbox from the accessibility tree,
    // the trigger included, and the role query would stop matching.
    const trigger = page.locator('[role="combobox"][aria-label="Group by"]')
    await trigger.click()
    const listbox = page.locator('#overlays [role="listbox"]')
    await expect(listbox).toBeVisible()
    await expect(trigger).toHaveAttribute('aria-expanded', 'true')
    await expectNoSeriousViolations(page, '#overlays')
    const options = listbox.getByRole('option')
    expect(await options.count()).toBeGreaterThan(1)
    // The selected option holds focus on open; arrows walk from there.
    await expect(page.locator('#overlays [role="option"][aria-selected="true"]')).toBeFocused()
    await page.keyboard.press('ArrowDown')
    await expect(page.locator('#overlays [role="option"]:focus')).toHaveCount(1)
    await page.keyboard.press('Escape')
    await expect(listbox).toHaveCount(0)
    await expect(trigger).toBeFocused()
  })

  test('a searchable dropdown filters from its input and closes on Escape', async ({ page }) => {
    await gotoAndSettle(page, '/profile/settings/language')
    const trigger = page.getByRole('button', { name: 'Timezone' })
    await trigger.click()
    const surface = page.locator('#overlays [role="dialog"]')
    await expect(surface).toBeVisible()
    const input = surface.getByRole('textbox')
    await expect(input).toBeFocused()
    await expectNoSeriousViolations(page, '#overlays')
    const before = await surface.getByRole('option').count()
    expect(before).toBeGreaterThan(10)
    await input.fill('sydney')
    await expect(surface.getByRole('option')).toHaveCount(1)
    // The filter carries aria-activedescendant to the highlighted row.
    await expect(input).toHaveAttribute('aria-activedescendant', /.+/)
    await page.keyboard.press('Escape')
    await expect(surface).toHaveCount(0)
    await expect(trigger).toBeFocused()
  })

  test('tab bars walk with the arrow keys and activate on focus', async ({ page }) => {
    await gotoAndSettle(page, '/inbox')
    const tablist = page.getByRole('tablist').first()
    await expect(tablist).toBeVisible()
    const tabs = tablist.getByRole('tab')
    expect(await tabs.count()).toBe(3)
    await expect(tabs.first()).toHaveAttribute('aria-selected', 'true')
    // Entering the group lands on the active tab; arrows move and select.
    await tabs.first().focus()
    await page.keyboard.press('ArrowRight')
    await expect(tabs.nth(1)).toBeFocused()
    await expect(tabs.nth(1)).toHaveAttribute('aria-selected', 'true')
    await page.keyboard.press('End')
    await expect(tabs.last()).toBeFocused()
    await expect(tabs.last()).toHaveAttribute('aria-selected', 'true')
    await page.keyboard.press('Home')
    await expect(tabs.first()).toHaveAttribute('aria-selected', 'true')
    await expectNoSeriousViolations(page, '[role="tablist"]')
  })

  test('a switch is named by its label and toggles from the keyboard and the label', async ({ page }) => {
    await gotoAndSettle(page, '/profile/settings/appearance')
    const sw = page.getByRole('switch', { name: 'Compact view' })
    await expect(sw).toBeVisible()
    const before = await sw.getAttribute('aria-checked')
    await sw.focus()
    await page.keyboard.press('Space')
    await expect(sw).toHaveAttribute('aria-checked', before === 'true' ? 'false' : 'true')
    // The visible label is a real <label for>, so it toggles the switch too.
    await page.getByText('Compact view', { exact: true }).click()
    await expect(sw).toHaveAttribute('aria-checked', before ?? 'false')
    await expectNoSeriousViolations(page, 'main')
  })

  test('a checkbox reports its state and a radio group walks with the arrows', async ({ page }) => {
    await gotoAndSettle(page, '/tickets')
    // Row checkboxes show on hover; the header's select-all appears once
    // a row is selected (bulk mode).
    await page.locator('table tbody tr').first().hover()
    const rowBox = page.getByRole('checkbox', { name: /^Select ticket/ }).first()
    await expect(rowBox).toBeVisible()
    await expect(rowBox).toHaveAttribute('aria-checked', 'false')
    await rowBox.click()
    await expect(rowBox).toHaveAttribute('aria-checked', 'true')
    const selectAll = page.getByRole('checkbox', { name: 'Select all visible tickets' })
    await expect(selectAll).toBeVisible()
    await expect(selectAll).toHaveAttribute('aria-checked', 'mixed')
    await selectAll.focus()
    await page.keyboard.press('Space')
    await expect(selectAll).toHaveAttribute('aria-checked', 'true')
    await page.keyboard.press('Escape')

    const density = page.getByRole('radiogroup', { name: 'Row density' })
    await expect(density).toBeVisible()
    const radios = density.getByRole('radio')
    await expect(radios).toHaveCount(3)
    const checked = density.locator('[role="radio"][aria-checked="true"]')
    await expect(checked).toHaveCount(1)
    await checked.focus()
    // Arrows move focus and select together, like native radios. Reka
    // checks the newly focused radio while the arrow key is still held,
    // so hold it for a beat rather than a synthetic instant press.
    await page.keyboard.down('ArrowRight')
    await page.waitForTimeout(80)
    await page.keyboard.up('ArrowRight')
    await expect(density.locator('[role="radio"]:focus')).toHaveAttribute('aria-checked', 'true')
    await expect(checked).toHaveCount(1)
    await expectNoSeriousViolations(page, '[role="radiogroup"]')
  })

  test('an inline edit opens from keyboard focus, commits on Enter and cancels on Escape', async ({ page }) => {
    await gotoAndSettle(page, '/tickets')
    await page.locator('table tbody tr').first().dblclick()
    await page.waitForURL(/\/tickets\/\d+/)
    await page.waitForTimeout(3000)

    const editor = page.getByRole('textbox', { name: 'Enter ticket title...' })
    await expect(editor).toBeHidden()
    // The preview is a tab stop; focusing it is what opens the editor.
    const preview = page.locator('header [data-placeholder-shown][tabindex="0"]').first()
    const shown = await preview.textContent()
    await preview.focus()
    await expect(editor).toBeVisible()
    await expect(editor).toBeFocused()
    await expect(editor).toHaveValue(shown?.trim() ?? '')
    await page.keyboard.press('Escape')
    await expect(editor).toBeHidden()
    await expect(preview).toHaveText(shown?.trim() ?? '')
    await expectNoSeriousViolations(page, 'header')
  })

  test('a sidebar section is one heading button that controls its panel', async ({ page }) => {
    await gotoAndSettle(page, '/tickets')
    const trigger = page.getByRole('button', { name: 'Recent tickets' })
    await expect(trigger).toBeVisible()
    const expanded = (await trigger.getAttribute('aria-expanded')) === 'true'
    const panelId = await trigger.getAttribute('aria-controls')
    expect(panelId).toBeTruthy()
    await trigger.focus()
    await page.keyboard.press('Space')
    await expect(trigger).toHaveAttribute('aria-expanded', expanded ? 'false' : 'true')
    // The panel exists whenever the section is open, and carries the id
    // the trigger points at.
    if (!expanded) await expect(page.locator(`[id="${panelId}"]`)).toBeVisible()
    await page.keyboard.press('Space')
    await expect(trigger).toHaveAttribute('aria-expanded', expanded ? 'true' : 'false')
    await expectNoSeriousViolations(page, 'nav')
  })

  test('a toast lands in a named region, is announced, pauses on hover and closes on Escape', async ({ page }) => {
    await gotoAndSettle(page, '/profile/settings/appearance')
    const sw = page.getByRole('switch', { name: 'Color blind friendly mode' })
    await sw.click()
    const region = page.locator('#overlays [role="region"]')
    await expect(region).toHaveAttribute('aria-label', /Notifications/)
    const toast = region.locator('li[data-state="open"]').first()
    await expect(toast).toBeVisible()
    // Hover at once: it pauses the five-second timer for the rest of the
    // test, and proves the pause (still there well past its life).
    await toast.hover()
    await expect(toast).toContainText('Color blind friendly mode')
    // Reka mirrors the toast into a live region for screen readers.
    await expect(page.locator('[role="alert"][aria-live]').first()).toBeAttached()
    await expectNoSeriousViolations(page, '#overlays')
    await page.waitForTimeout(5500)
    await expect(toast).toBeVisible()
    // Escape on the focused toast closes it.
    await toast.focus()
    await page.keyboard.press('Escape')
    await expect(region.locator('li[data-state="open"]')).toHaveCount(0)
    // Put the setting back.
    await sw.click()
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
