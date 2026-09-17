import { test, expect, type Page } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'
import { gotoAndSettle } from './helpers'

/**
 * Accessibility floor for the surfaces being moved onto Reka UI. Each
 * migration PR adds its surface here, so the floor only rises.
 *
 * axe runs with the WCAG 2.x A/AA tags and fails on serious or critical
 * impact only: moderate findings (contrast on tertiary text and the like)
 * are tracked separately, and a floor that fails on everything is a floor
 * nobody keeps.
 */
async function expectNoSeriousViolations(page: Page, scope?: string): Promise<void> {
  let builder = new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
  if (scope) builder = builder.include(scope)
  const results = await builder.analyze()
  const serious = results.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical')
  expect(
    serious.map((v) => `${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(' ')).join(', ')})`),
  ).toEqual([])
}

test.describe('accessibility floor', () => {
  test.skip(({ hasTouch }) => hasTouch, 'hover and focus are pointer/keyboard concerns')

  test('icon buttons expose a tooltip on focus and describe the trigger', async ({ page }) => {
    await gotoAndSettle(page, '/tickets')
    const trigger = page.locator('button[data-icon-only]').first()
    await expect(trigger).toBeVisible()
    await trigger.focus()
    const tooltip = page.getByRole('tooltip')
    await expect(tooltip).toBeVisible()
    await expect(tooltip).toHaveText(await trigger.getAttribute('aria-label') ?? '')
    // aria-describedby is set on open so screen readers read the hint.
    await expect(trigger).toHaveAttribute('aria-describedby', /.+/)
    await page.keyboard.press('Escape')
    await expect(tooltip).toBeHidden()
    // Hover opens it too, after the provider delay.
    await trigger.hover()
    await expect(page.getByRole('tooltip')).toBeVisible()
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
