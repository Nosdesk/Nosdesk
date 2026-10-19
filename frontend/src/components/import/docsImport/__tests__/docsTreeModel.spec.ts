import { describe, expect, it } from 'vitest'
import type { DocsTreePage, DocsTreeView } from '@nosdesk/core/services/docsImportService'

import {
  buildTree,
  collectionGroups,
  globMatch,
  informativeFile,
  isMixed,
  isOff,
  leftOutBy,
  optionsPayload,
  patternsProblem,
  shortPath,
  splitPatterns,
  tickPatterns,
  toggle,
  treeCounts,
  type PagesChoices,
} from '../docsTreeModel'

function page(index: number, title: string, parent: number | null, file: string | null, folder: string | null = null): DocsTreePage {
  return { index, title, path: file ?? folder!, file, folder, parent, order: index, collection: parent == null ? 0 : null }
}

// Onboarding (a page with a folder) > Day one, Laptop; Network (folder only)
// > VPN, Archive (folder only) > Old plan.
const PAGES: DocsTreePage[] = [
  page(0, 'Onboarding', null, 'Onboarding.md', 'Onboarding'),
  page(1, 'Day one', 0, 'Onboarding/Day one.md'),
  page(2, 'Laptop', 0, 'Onboarding/Laptop.md'),
  page(3, 'Network', null, null, 'Network'),
  page(4, 'VPN', 3, 'Network/VPN.md'),
  page(5, 'Archive', 3, null, 'Network/Archive'),
  page(6, 'Old plan', 5, 'Network/Archive/Old plan.md'),
]

const tree = () => buildTree(PAGES)
const node = (path: string) => tree().byPath.get(path)!

const CHOICES: PagesChoices = {
  profile: null,
  mode: 'new',
  collectionName: '',
  collectionId: null,
  underPageId: null,
  status: 'drafts',
  webImages: 'download',
  duplicates: 'skip',
  typed: [],
}

describe('buildTree', () => {
  it('nests pages under their parents in order, with levels', () => {
    const t = tree()
    expect(t.roots.map((n) => n.page.title)).toEqual(['Onboarding', 'Network'])
    expect(t.all.map((n) => [n.page.title, n.level])).toEqual([
      ['Onboarding', 0],
      ['Day one', 1],
      ['Laptop', 1],
      ['Network', 0],
      ['VPN', 1],
      ['Archive', 1],
      ['Old plan', 2],
    ])
  })
})

describe('globMatch, as the server matches', () => {
  it.each([
    ['**/archive/**', 'docs/archive/old.md', true],
    ['**/archive/**', 'archive/old.md', true],
    ['**/archive/**', 'docs/archived.md', false],
    ['drafts', 'a/drafts/b.md', true],
    ['*.tmp.md', 'a/x.tmp.md', true],
    ['docs/*.md', 'docs/a.md', true],
    ['docs/*.md', 'docs/sub/a.md', false],
    ['**/archive/**', 'docs/xarchive/old.md', false],
    ['docs/**', 'docs/a/b/c.md', true],
    ['?.md', 'a/b.md', true],
    ['/README.md', 'README.md', true],
    ['/README.md', 'sub/README.md', false],
    ['README.md', 'sub/README.md', true],
  ])('%s against %s is %s', (pattern, path, want) => {
    expect(globMatch(pattern, path)).toBe(want)
  })
})

describe('ticks', () => {
  it('children follow an unticked parent, and the parent shows a dash when some child is off', () => {
    let off = toggle(node('Onboarding/Day one.md'), new Set())
    expect(isMixed(node('Onboarding.md'), off)).toBe(true)
    expect(isOff(node('Onboarding/Day one.md'), off)).toBe(true)

    off = toggle(node('Network'), off)
    expect(isOff(node('Network/Archive/Old plan.md'), off)).toBe(true)
    expect(treeCounts(tree(), off, [])).toEqual({ total: 7, ticked: 2, unticked: 5, leftOut: 0 })
  })

  it('a mixed tick ticks everything under it, and an unticked one comes back', () => {
    let off = toggle(node('Onboarding/Day one.md'), new Set())
    off = toggle(node('Onboarding.md'), off)
    expect([...off]).toEqual([])
    off = toggle(node('Onboarding.md'), off)
    expect([...off]).toEqual(['Onboarding.md'])
    expect([...toggle(node('Onboarding.md'), off)]).toEqual([])
  })

  it('turns unticked branches into exact patterns, and reads them back', () => {
    const off = new Set(['Onboarding.md', 'Network/VPN.md'])
    const patterns = tickPatterns(tree(), off)
    expect(patterns).toEqual(['/Onboarding.md', '/Onboarding/**', '/Network/VPN.md'])
    // Every page of the branch is matched by them, nothing else.
    const matched = PAGES.filter((p) => p.file && patterns.some((g) => globMatch(g, p.file!))).map((p) => p.title)
    expect(matched).toEqual(['Onboarding', 'Day one', 'Laptop', 'VPN'])

    const back = splitPatterns(tree(), [...patterns, '**/archive/**'])
    expect([...back.off].sort()).toEqual(['Network/VPN.md', 'Onboarding.md'])
    expect(back.typed).toEqual(['**/archive/**'])
  })

  it('refuses more patterns than the server takes', () => {
    expect(patternsProblem(Array.from({ length: 101 }, (_, i) => `/${i}.md`))).toBe('too_many')
    expect(patternsProblem(['x'.repeat(501)])).toBe('too_long')
    expect(patternsProblem(['**/archive/**'])).toBeNull()
  })
})

describe('leave-out patterns', () => {
  it('grey a page by its file, and a section page when its whole folder goes', () => {
    expect(leftOutBy(node('Network/Archive/Old plan.md'), ['**/archive/**'])).toBeNull()
    expect(leftOutBy(node('Network/Archive/Old plan.md'), ['Archive'])).toBe('Archive')
    expect(leftOutBy(node('Network/Archive'), ['Network/Archive/**'])).toBe('Network/Archive/**')
    expect(leftOutBy(node('Onboarding.md'), ['Onboarding/**'])).toBeNull()
    expect(treeCounts(tree(), new Set(), ['Archive']).leftOut).toBe(2)
  })
})

describe('optionsPayload', () => {
  it('sends the ticks and typed patterns at Check, and none when reading the tree again', () => {
    const choices = { ...CHOICES, typed: ['**/archive/**'], collectionName: ' IT Handbook ' }
    expect(optionsPayload(choices, ['/Onboarding.md'], 'check')).toEqual({
      collections: 'new',
      collection_name: 'IT Handbook',
      status: 'drafts',
      web_images: 'download',
      duplicates: 'skip',
      leave_out: ['**/archive/**', '/Onboarding.md'],
    })
    expect(optionsPayload(choices, ['/Onboarding.md'], 'reread').leave_out).toEqual([])
  })

  it('names the collection and page only for an existing collection, and the profile when forced', () => {
    const existing = optionsPayload(
      { ...CHOICES, mode: 'existing', collectionId: 7, underPageId: 12, collectionName: 'ignored', profile: 'obsidian' },
      [],
      'check',
    )
    expect(existing).toMatchObject({ collections: 'existing', collection_id: 7, under_page_id: 12, profile: 'obsidian' })
    expect(existing).not.toHaveProperty('collection_name')
    const perFolder = optionsPayload({ ...CHOICES, mode: 'per_top_level_folder', collectionId: 7 }, [], 'check')
    expect(perFolder).not.toHaveProperty('collection_id')
    expect(perFolder).not.toHaveProperty('profile')
  })
})

describe('collectionGroups', () => {
  const view = { collections: ['Onboarding', 'Network'] } as DocsTreeView
  it('heads the tree with the chosen destination', () => {
    expect(collectionGroups(tree(), view, { mode: 'new', collectionName: '' }, null)[0]).toMatchObject({ name: 'Onboarding', kind: 'new' })
    expect(collectionGroups(tree(), view, { mode: 'new', collectionName: 'Handbook' }, null)[0].name).toBe('Handbook')
    expect(
      collectionGroups(tree(), view, { mode: 'existing', collectionName: '' }, { name: 'IT', under: 'Network' })[0].name,
    ).toBe('IT / Network')
  })

  it('splits per collection by the pages the server placed', () => {
    const pages = PAGES.map((p) => (p.index === 3 ? { ...p, collection: 1 } : p))
    const groups = collectionGroups(buildTree(pages), view, { mode: 'per_top_level_folder', collectionName: '' }, null)
    expect(groups.map((g) => [g.name, g.nodes.map((n) => n.page.title)])).toEqual([
      ['Onboarding', ['Onboarding']],
      ['Network', ['Network']],
    ])
  })
})

describe('file names', () => {
  it('shows a file only when it says something the title does not', () => {
    expect(informativeFile(page(0, 'VPN setup', null, 'Net/VPN setup 9f3c1e2d4b5a6c7d8e9f0a1b2c3d4e5f.md'))).toBeNull()
    expect(informativeFile(page(0, 'Getting started', null, 'docs/01-getting-started.md'))).toBeNull()
    expect(informativeFile(page(0, 'Wi-Fi and guest access', null, 'Net/Wifi 1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d.md'))).toBe(
      'Wifi 1a2b3c4d.md',
    )
    expect(informativeFile(page(0, 'Network', null, null, 'Network'))).toBeNull()
    expect(shortPath('IT 9f3c1e2d4b5a6c7d8e9f0a1b2c3d4e5f/VPN 1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d.md')).toBe('IT 9f3c1e2d/VPN 1a2b3c4d.md')
  })
})
