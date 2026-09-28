import { describe, expect, it } from 'vitest'

import { portalTheme, routeForSubPage } from './teams'

describe('Teams tab', () => {
  it('opens a deep link at its ticket or approval', () => {
    expect(routeForSubPage('ticket-42')).toBe('/tickets/42')
    expect(routeForSubPage('approval-7')).toBe('/approvals/7')
    expect(routeForSubPage(null)).toBe('/tickets')
    expect(routeForSubPage('ticket-42/../admin')).toBe('/tickets')
  })

  it('follows the Teams theme', () => {
    expect(portalTheme('default')).toBe('light')
    expect(portalTheme('dark')).toBe('dark')
    expect(portalTheme('contrast')).toBe('pure-black')
  })
})
