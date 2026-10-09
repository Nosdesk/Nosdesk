import type { Node as PMNode, Schema } from 'prosemirror-model';
import type * as Y from 'yjs';
import { initProseMirrorDoc } from 'y-prosemirror';

type Mapping = ReturnType<typeof initProseMirrorDoc>['mapping'];

/**
 * The document a live collaborative editor starts from.
 *
 * Content that arrives later, from IndexedDB or another client, must never
 * become the selection. For an empty fragment `initProseMirrorDoc` builds a
 * document with no blocks, which the schema forbids and which has no caret
 * position, so its selection is an AllSelection; y-prosemirror then keeps that
 * selection across the content's arrival, leaving the whole note selected,
 * and the next keystroke replaced it. An empty note starts from the schema's
 * own empty document instead (one empty paragraph), with a caret in it.
 * y-prosemirror writes nothing to Yjs for that paragraph until someone types.
 */
export function initialEditorDoc(
  fragment: Y.XmlFragment,
  schema: Schema,
): { doc: PMNode; mapping: Mapping } {
  if (fragment.length === 0) {
    const doc = schema.topNodeType.createAndFill();
    if (doc) return { doc, mapping: new Map() };
  }
  const { doc, mapping } = initProseMirrorDoc(fragment, schema);
  return { doc, mapping };
}
