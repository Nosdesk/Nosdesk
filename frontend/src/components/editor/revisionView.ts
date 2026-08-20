/**
 * Detached read-only viewer for a stored revision.
 *
 * ## Why this exists
 *
 * Revision viewing used to work by calling `editorView.updateState()` on the
 * LIVE editor with a plugin list that omitted `ySyncPlugin`. That is a data-loss
 * bug, not a style problem:
 *
 *   1. ProseMirror's `updatePluginViews` destroys every plugin view whenever
 *      `prevState.plugins != this.state.plugins` (prosemirror-view/dist/index.js).
 *      Swapping the plugin list therefore ran `ySyncPlugin`'s `destroy`, which
 *      calls `binding.destroy()` -> `prosemirrorView = null` and
 *      `type.unobserveDeep(...)`.
 *   2. With the binding unobserved, every remote edit arriving while a revision
 *      was on screen never reached the ProseMirror document.
 *   3. Exiting restored an `EditorState` captured BEFORE those edits. The binding
 *      re-initialised against the stale document, and the next change ran
 *      `updateYFragment`, which diffed stale ProseMirror against the advanced Yjs
 *      fragment, deleted the collaborators' work and broadcast the deletion.
 *
 * So: never touch the live view. This mounts a second, throwaway `EditorView`
 * over a scratch `Y.Doc` and renders it in an overlay. The live editor stays
 * mounted, bound and receiving updates the whole time, and exiting is just a
 * teardown of things nobody else references.
 *
 * The scratch doc is deliberately provider-free. y-prosemirror's node-building
 * catch block deletes the offending item from whatever doc it is rendering and
 * lets that deletion propagate; keeping every doc we touch here detached means a
 * render failure can only damage something we are about to throw away.
 */
import { EditorState, type Plugin } from 'prosemirror-state';
import { EditorView } from 'prosemirror-view';
import type { Schema } from 'prosemirror-model';
import * as Y from 'yjs';
import { initProseMirrorDoc } from 'y-prosemirror';

/** The Yjs root a Nosdesk collaborative document lives under. */
const ROOT_FRAGMENT = 'prosemirror';

export interface RevisionViewHandle {
  /** The detached view, exposed for tests and for reading text out. */
  readonly view: EditorView;
  /** Plain text of the revision, for copy-out and assertions. */
  text(): string;
  /** Idempotent. */
  destroy(): void;
}

/**
 * Decode the base64 payload the revision endpoints return.
 *
 * Both `GET /collaboration/tickets/{id}/revisions/{n}` and the `/docs/` sibling
 * return the column base64-encoded under `yjs_document_content`, and both store a
 * full v1 document update despite the documentation column being named
 * `yjs_document_snapshot`.
 */
export function decodeRevisionBytes(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/**
 * Render a stored revision into `mount`, detached from the live document.
 *
 * Throws if the update cannot be decoded, having cleaned up after itself, so the
 * caller can surface the failure without leaving a half-built view behind.
 */
export function mountRevisionView(opts: {
  mount: HTMLElement;
  schema: Schema;
  /** Full Yjs v1 document update for the revision. */
  updateBytes: Uint8Array;
  /**
   * Plugins for the detached view. Supplied by the caller rather than imported
   * here, for two reasons: the node-view plugins reach the router, which would
   * drag every view into anything importing this module, and the fidelity of a
   * historical render is the caller's decision.
   *
   * Pass only what renders. Anything that can dispatch a transaction (keymaps,
   * input rules, drop cursor, image upload, or any Yjs plugin) does not belong
   * on a static document.
   */
  plugins?: readonly Plugin[];
}): RevisionViewHandle {
  const { mount, schema, updateBytes, plugins = [] } = opts;

  // gc:false mirrors the live document and the server (`skip_gc: true`), so a
  // revision carrying tombstones decodes with its history intact.
  const scratchDoc = new Y.Doc({ gc: false });
  let view: EditorView | null = null;

  try {
    Y.applyUpdate(scratchDoc, updateBytes);
    const fragment = scratchDoc.getXmlFragment(ROOT_FRAGMENT);
    const { doc } = initProseMirrorDoc(fragment, schema);

    view = new EditorView(mount, {
      state: EditorState.create({ doc, schema, plugins: [...plugins] }),
      // Set as a direct prop rather than relying on a plugin, so the view is
      // non-editable from its very first render.
      editable: () => false,
      attributes: { class: 'revision-view-content', 'aria-readonly': 'true' },
    });
  } catch (error) {
    view?.destroy();
    scratchDoc.destroy();
    throw error;
  }

  const liveView = view;
  let destroyed = false;

  return {
    view: liveView,
    text: () => liveView.state.doc.textBetween(0, liveView.state.doc.content.size, '\n', '\n'),
    destroy() {
      if (destroyed) return;
      destroyed = true;
      liveView.destroy();
      scratchDoc.destroy();
    },
  };
}
