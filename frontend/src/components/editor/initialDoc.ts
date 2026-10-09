import type { Node as PMNode, Schema } from 'prosemirror-model';
import type * as Y from 'yjs';
import { initProseMirrorDoc } from 'y-prosemirror';

type Mapping = ReturnType<typeof initProseMirrorDoc>['mapping'];

/**
 * The document a live collaborative editor starts from.
 *
 * Content that arrives later, from IndexedDB or another client, must never
 * become the selection. A document with no blocks, which `initProseMirrorDoc`
 * builds from an empty fragment or from one whose blocks this build can't
 * show, is forbidden by the schema and has no caret position, so its selection
 * is an AllSelection; y-prosemirror then keeps that selection across the
 * content's arrival, leaving the whole note selected, and the next keystroke
 * replaced it. Such a note starts from the schema's own empty document (one
 * empty paragraph) instead, with a caret in it. y-prosemirror writes nothing to
 * Yjs for that paragraph until someone types.
 */
export function initialEditorDoc(
  fragment: Y.XmlFragment,
  schema: Schema,
): { doc: PMNode; mapping: Mapping } {
  const { doc, mapping } = initProseMirrorDoc(fragment, schema);
  if (doc.childCount > 0) return { doc, mapping };
  const empty = schema.topNodeType.createAndFill();
  return empty ? { doc: empty, mapping: new Map() } : { doc, mapping };
}
