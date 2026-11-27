import { describe, expect, it } from 'vitest'
import { importedAuthor } from '../importedAuthor'

const imported = (author_name: string | null, name: string, source = 'zendesk') => ({
  channel_metadata: { import: { source, author_name, author_email: 'pat@example.com' } },
  user: { name },
})

describe('importedAuthor', () => {
  it('names the source author of a reply credited to the placeholder', () => {
    expect(importedAuthor(imported('Pat Lee', 'Unknown (Zendesk)'))).toEqual({
      name: 'Pat Lee',
      source: 'Zendesk',
    })
  })

  it('names them when the account goes by another name here', () => {
    expect(importedAuthor(imported('Pat Lee', 'Patricia Lee'))?.name).toBe('Pat Lee')
  })

  it('says nothing when the name is the one shown, or there is none', () => {
    expect(importedAuthor(imported('Pat Lee', 'Pat Lee'))).toBeNull()
    expect(importedAuthor(imported(' ', 'Unknown (Zendesk)'))).toBeNull()
    expect(importedAuthor(imported(null, 'Unknown (Zendesk)'))).toBeNull()
    expect(importedAuthor({ channel_metadata: null, user: { name: 'A' } })).toBeNull()
    expect(
      importedAuthor({ channel_metadata: { forwarded_by_user_uuid: 'x' }, user: { name: 'A' } }),
    ).toBeNull()
  })

  it('keeps an unknown source by its id and never returns the address', () => {
    const got = importedAuthor(imported('Pat Lee', 'Unknown', 'otherdesk'))
    expect(got).toEqual({ name: 'Pat Lee', source: 'otherdesk' })
    expect(JSON.stringify(got)).not.toContain('@')
  })
})
