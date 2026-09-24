/**
 * Render a page's saved content (a Yjs update) to sanitised HTML, through the
 * editor's own schema: Yjs XmlFragment -> ProseMirror JSON -> Node -> DOM, so
 * each node's `toDOM` decides the markup and nothing is re-implemented here.
 *
 * Used where the live editor isn't: embeds of another page, and public docs
 * on the portal.
 */
import { DOMSerializer, Node as ProseMirrorNode } from 'prosemirror-model'
import * as Y from 'yjs'
import { schema } from './schema'
import { sanitiseHtml } from '@/composables/useSanitise'

export interface SavedDocOptions {
  /** Node types to leave out entirely (with their content). */
  omit?: readonly string[]
}

/** Decode a base64 string (the embed content endpoint's encoding). */
export function base64ToBytes(base64: string): Uint8Array {
  return Uint8Array.from(atob(base64), (c) => c.charCodeAt(0))
}

/**
 * HTML for a saved Yjs document. Returns '' for an empty page; throws if the
 * content can't be rendered (e.g. a node type this editor doesn't know), so a
 * caller can tell "empty" from "can't show".
 */
export function savedDocToHtml(update: Uint8Array, options: SavedDocOptions = {}): string {
  const ydoc = new Y.Doc()
  try {
    Y.applyUpdate(ydoc, update)
    return xmlFragmentToHtml(ydoc.getXmlFragment('prosemirror'), new Set(options.omit ?? []))
  } finally {
    ydoc.destroy()
  }
}

function xmlFragmentToHtml(fragment: Y.XmlFragment, omit: ReadonlySet<string>): string {
  const content = fragment.toArray().flatMap((child) => {
    if (!(child instanceof Y.XmlElement)) return []
    const json = xmlElementToJSON(child, omit)
    return json ? [json] : []
  })
  if (content.length === 0) return ''

  const doc = ProseMirrorNode.fromJSON(schema, { type: 'doc', content })
  const dom = DOMSerializer.fromSchema(schema).serializeFragment(doc.content)
  const wrapper = document.createElement('div')
  wrapper.appendChild(dom)
  return sanitiseHtml(wrapper.innerHTML)
}

 
function xmlElementToJSON(element: Y.XmlElement, omit: ReadonlySet<string>): any {
  const type = element.nodeName
  if (!type || type === 'undefined' || omit.has(type)) return null

  const attrs: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(element.getAttributes())) {
    if (key !== 'ychange') attrs[key] = value
  }

   
  const content: any[] = []
  for (const child of element.toArray()) {
    if (child instanceof Y.XmlElement) {
      const json = xmlElementToJSON(child, omit)
      if (json) content.push(json)
    } else if (child instanceof Y.XmlText) {
      for (const delta of child.toDelta()) {
         
        const textNode: any = { type: 'text', text: delta.insert }
        if (delta.attributes) {
          textNode.marks = Object.entries(delta.attributes)
            .filter(([k]) => k !== 'ychange')
            .map(([markType, value]) =>
              typeof value === 'object' && value !== null
                ? { type: markType, attrs: value }
                : { type: markType },
            )
        }
        if (textNode.text) content.push(textNode)
      }
    }
  }

   
  const node: any = { type }
  if (Object.keys(attrs).length > 0) node.attrs = attrs
  if (content.length > 0) node.content = content
  return node
}
