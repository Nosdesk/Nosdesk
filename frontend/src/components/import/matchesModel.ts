/**
 * Saved matches page: the rules behind its one table. Entries arrive in
 * field order; each field is a section with a count, and a field answered
 * in a context (a model under its manufacturer, an attribute under a
 * kind) gets a sub-section per context inside its own section. Search
 * filters rows, hides sections and sub-sections it empties, and reports
 * "4 of 15". The hub card's examples come from the same order.
 */
import type { SavedMatch } from '@nosdesk/core/services/importService'

/** Above this many entries the page offers search. */
export const SEARCH_FROM = 10

/** How many examples the hub card shows per type. */
export const EXAMPLES = 3

export interface SubSection {
  /** The context key; empty for none. */
  context: string
  /** Its name, or null for none. */
  label: string | null
  entries: SavedMatch[]
}

export interface Section {
  field: string
  /** Entries in the section, before search. */
  total: number
  /** Entries search leaves. */
  shown: number
  /** One flat list when the field has no contexts. */
  flat: boolean
  subs: SubSection[]
}

export interface Sections {
  sections: Section[]
  total: number
  shown: number
}

/** Whether an answer drops what the file says. */
export function isDiscard(e: SavedMatch): boolean {
  return e.outcome !== 'link'
}

/** Text search compares: lowercased, spaces collapsed. */
export function normalize(text: string): string {
  return text.toLowerCase().split(/\s+/).filter(Boolean).join(' ')
}

/**
 * The page's sections. `describe` gives the words a row shows for an
 * entry (field, context, what it becomes) so search can match them.
 */
export function sections(
  entries: SavedMatch[],
  query = '',
  describe: (e: SavedMatch) => string = () => '',
): Sections {
  const q = normalize(query)
  const hit = (e: SavedMatch) =>
    !q || normalize(`${e.text} ${describe(e)}`).includes(q)
  const out: Section[] = []
  for (const e of entries) {
    let section = out.find((s) => s.field === e.field)
    if (!section) {
      section = { field: e.field, total: 0, shown: 0, flat: true, subs: [] }
      out.push(section)
    }
    section.total += 1
    let sub = section.subs.find((s) => s.context === e.context)
    if (!sub) {
      sub = { context: e.context, label: e.context_label ?? null, entries: [] }
      section.subs.push(sub)
    }
    if (hit(e)) {
      section.shown += 1
      sub.entries.push(e)
    }
  }
  for (const section of out) {
    section.flat =
      section.subs.length === 1 && section.subs[0].context === '' && section.field !== 'model'
    section.subs = section.subs.filter((s) => s.entries.length > 0)
  }
  const total = entries.length
  const visible = out.filter((s) => s.shown > 0)
  return { sections: visible, total, shown: visible.reduce((n, s) => n + s.shown, 0) }
}

/** The hub card's examples: the first few that keep data, in page order. */
export function examples(entries: SavedMatch[], count = EXAMPLES): SavedMatch[] {
  return entries.filter((e) => !isDiscard(e)).slice(0, count)
}

/** A person's first name, for the narrow Saved by column. */
export function firstName(name: string): string {
  return name.trim().split(/\s+/)[0] ?? name
}
