import { afterEach, describe, expect, it } from 'vitest'
import { snapshotPluginTheme } from './theme'

const root = document.documentElement

afterEach(() => root.style.removeProperty('--user-text-scale'))

describe('snapshotPluginTheme', () => {
  it('passes the host text size to the plugin, with the header size it implies', () => {
    expect(snapshotPluginTheme('light', 'light').tokens['text-scale']).toBe('1')
    root.style.setProperty('--user-text-scale', '1.5')
    const { tokens } = snapshotPluginTheme('light', 'light')
    expect(tokens['text-scale']).toBe('1.5')
    expect(tokens['header-font-size']).toBe('19.5px')
  })
})
