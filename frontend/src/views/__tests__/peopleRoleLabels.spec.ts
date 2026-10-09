import { describe, expect, it } from 'vitest'
import source from '../UsersListView.vue?raw'

// People see a role by its name (technician reads "Agent"), never the stored
// token. Every role badge in the People view takes a label, and no role is
// printed raw.
describe('People view role labels', () => {
  it('labels every role badge', () => {
    const badges = source.match(/<StatusBadgeCell[^>]*type="role"[^>]*>/g) ?? []
    expect(badges.length).toBeGreaterThan(0)
    for (const badge of badges) expect(badge).toContain(':label=')
  })

  it('prints no role token', () => {
    expect(source).not.toMatch(/\{\{\s*effectiveRole\(/)
  })
})
