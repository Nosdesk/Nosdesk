import { describe, expect, it } from 'vitest'
import { DOMSerializer } from 'prosemirror-model'
import { schema, safeLinkHref } from '../schema'

describe('editor links', () => {
  it('keep web, mail, phone and same-origin addresses', () => {
    for (const href of ['https://nosdesk.com/a?b=1', 'http://x.test', 'mailto:a@b.test', 'tel:+61 2 9999', '/tickets/7', '#top', 'docs/page']) {
      expect(safeLinkHref(href)).toBe(href)
    }
  })

  it('drop script and data addresses, however they are dressed up', () => {
    for (const href of ['javascript:alert(1)', ' JavaScript:alert(1)', 'java\tscript:alert(1)', 'data:text/html,<script>', 'vbscript:x']) {
      expect(safeLinkHref(href)).toBeNull()
    }
  })

  it('render a script link written into the document without its address', () => {
    const mark = schema.marks.link.create({ href: 'javascript:alert(1)' })
    const doc = schema.node('doc', null, [schema.node('paragraph', null, [schema.text('click', [mark])])])
    const html = new XMLSerializer().serializeToString(
      DOMSerializer.fromSchema(schema).serializeFragment(doc.content),
    )
    expect(html).not.toContain('javascript')
    expect(html).toContain('click')
  })
})
