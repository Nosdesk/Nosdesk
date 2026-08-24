/**
 * ProseMirror Image Upload Plugin
 *
 * Intercepts paste and drop events containing images, uploads them, and
 * inserts a URL reference instead of a base64 dataURI. Large binary data in
 * the Yjs document is the failure mode this exists to prevent.
 *
 * ## Why the position is anchored twice
 *
 * The upload is async, so the insertion point must survive whatever happens to
 * the document while it is in flight. The usual ProseMirror recipe is a widget
 * decoration mapped through `tr.mapping`, and that handles local edits. It is
 * not enough here.
 *
 * y-prosemirror applies every REMOTE update as a single whole-document
 * `replace(0, doc.content.size, ...)` (see `ProsemirrorBinding._typeChanged` in
 * y-prosemirror/src/plugins/sync-plugin.js). Its step map reports every
 * interior position as deleted, and `WidgetType.map` returns null for a deleted
 * position, so a mapping-only placeholder is destroyed the moment any
 * collaborator types anywhere in the document.
 *
 * y-prosemirror solves the same problem for the local caret by anchoring it in
 * the CRDT: `getRelativeSelection` before the replace, `restoreRelativeSelection`
 * after. `yCursorPlugin` does the same for remote carets. We copy that: each
 * pending upload carries a Yjs relative position used to rebuild the widget on
 * y-sync transactions, and falls back to `tr.mapping` for local ones. Neither
 * alone is correct, because on a local edit the ProseMirror document changes
 * before ySyncPlugin pushes it into Yjs, leaving the relative position briefly
 * stale.
 */

import { Plugin, PluginKey, type EditorState } from 'prosemirror-state';
import { Decoration, DecorationSet, type EditorView } from 'prosemirror-view';
import { Fragment, Slice, type Node as PMNode } from 'prosemirror-model';
import {
  ySyncPluginKey,
  absolutePositionToRelativePosition,
  relativePositionToAbsolutePosition,
} from 'y-prosemirror';
import {
  uploadEditorImage,
  dataURLToFile,
  isDataURL,
  generateImageFilename,
  EditorImageUploadError,
  type EditorImageUploadResult,
} from '@/services/editorImageService';

/** Yjs relative position. Opaque here; only y-prosemirror interprets it. */
type RelativePosition = unknown;

interface PendingUpload {
  id: string;
  /** Text shown next to the spinner while the upload runs. */
  label: string;
  /** CRDT anchor, or null when the editor has no y-sync binding. */
  relPos: RelativePosition | null;
}

export interface ImageUploadPluginState {
  pending: PendingUpload[];
  decos: DecorationSet;
}

type ImageUploadMeta =
  | { kind: 'add'; id: string; pos: number; label: string; relPos: RelativePosition | null }
  | { kind: 'remove'; id: string };

export const imageUploadPluginKey = new PluginKey<ImageUploadPluginState>('imageUpload');

export interface ImageUploadPluginOptions {
  /** Collab doc id; the upload target is derived from it. */
  docId: string;
  /** Pre-translated placeholder label. The plugin is framework free, so the component translates. */
  uploadingLabel: (filename: string) => string;
  onUploadStart?: () => void;
  onUploadEnd?: () => void;
  onUploadError?: (error: unknown, file: { name: string }) => void;
}

function placeholderWidget(pos: number, pending: PendingUpload): Decoration {
  return Decoration.widget(
    pos,
    () => {
      const el = document.createElement('span');
      el.className = 'image-upload-placeholder';
      const spinner = document.createElement('span');
      spinner.className = 'image-upload-spinner';
      const label = document.createElement('span');
      label.textContent = pending.label;
      el.append(spinner, label);
      return el;
    },
    // `key` lets ProseMirror reuse the DOM across redraws so the spinner does
    // not restart. `side: 1` puts an insert at the same position before the
    // widget, which is what keeps a batch of images in paste order.
    { uploadId: pending.id, key: `image-upload-${pending.id}`, side: 1, ignoreSelection: true }
  );
}

/** Snapshot a position as a Yjs relative position, when a y-sync binding exists. */
function captureRelPos(state: EditorState, pos: number): RelativePosition | null {
  const ystate = ySyncPluginKey.getState(state);
  if (!ystate?.binding) return null;
  try {
    return absolutePositionToRelativePosition(pos, ystate.type, ystate.binding.mapping);
  } catch {
    // lib.js can throw on unusual trees; degrade to mapping-only anchoring.
    return null;
  }
}

/** Current position of a pending placeholder, or null when it is gone. */
function placeholderPos(state: EditorState, id: string): number | null {
  const found = imageUploadPluginKey
    .getState(state)
    ?.decos.find(undefined, undefined, (spec) => spec.uploadId === id);
  return found?.length ? found[0].from : null;
}

function newUploadId(): string {
  return (
    globalThis.crypto?.randomUUID?.() ??
    `up-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
  );
}

/** Add a placeholder at `pos` and start the upload. Returns a settled outcome. */
function startUpload(
  view: EditorView,
  file: File,
  alt: string,
  pos: number,
  options: ImageUploadPluginOptions
): PendingUploadHandle {
  const id = newUploadId();
  const relPos = captureRelPos(view.state, pos);

  view.dispatch(
    view.state.tr.setMeta(imageUploadPluginKey, {
      kind: 'add',
      id,
      pos,
      label: options.uploadingLabel(file.name),
      relPos,
    } satisfies ImageUploadMeta)
  );

  // Settle the promise here so a rejection never escapes as unhandled while
  // this entry waits its turn to be inserted.
  const settled = uploadEditorImage(file, { docId: options.docId }).then(
    (value) => ({ value }),
    (error: unknown) => ({ error })
  );

  return { id, file, alt, settled };
}

interface PendingUploadHandle {
  id: string;
  file: File;
  alt: string;
  settled: Promise<{ value?: EditorImageUploadResult; error?: unknown }>;
}

/** Insert one finished upload at wherever its placeholder ended up. */
function finishUpload(
  view: EditorView,
  handle: PendingUploadHandle,
  outcome: { value?: EditorImageUploadResult; error?: unknown },
  options: ImageUploadPluginOptions
): void {
  // The editor can unmount while an upload is in flight (paste, then navigate
  // away). Dispatching into a destroyed view throws, which would swallow the
  // error callback below.
  if (view.isDestroyed) return;

  const clearPlaceholder = () =>
    view.dispatch(
      view.state.tr.setMeta(imageUploadPluginKey, {
        kind: 'remove',
        id: handle.id,
      } satisfies ImageUploadMeta)
    );

  if (outcome.error !== undefined || !outcome.value) {
    clearPlaceholder();
    options.onUploadError?.(outcome.error, { name: handle.file.name });
    return;
  }

  const at = placeholderPos(view.state, handle.id);
  if (at === null) {
    // The anchor was deleted locally or by a collaborator while we uploaded.
    // Dropping the result beats guessing a position, but the upload DID happen,
    // so tell the user rather than leaving them with a silently missing image.
    clearPlaceholder();
    options.onUploadError?.(
      new EditorImageUploadError(
        'anchor-lost',
        `Anchor for ${handle.file.name} was removed during upload`
      ),
      { name: handle.file.name }
    );
    return;
  }

  const imageType = view.state.schema.nodes.image;
  if (!imageType) {
    clearPlaceholder();
    return;
  }

  view.dispatch(
    view.state.tr
      .insert(at, imageType.create({ src: outcome.value.url, alt: handle.alt, title: handle.alt }))
      .setMeta(imageUploadPluginKey, { kind: 'remove', id: handle.id } satisfies ImageUploadMeta)
  );
}

/**
 * Upload a batch at one position.
 *
 * Every placeholder is dispatched up front so each upload has an anchor before
 * any of them resolves, and the uploads themselves run concurrently. Insertion
 * is then serialised in paste order: all the widgets share a position, so
 * inserting one maps the rest to after it, and whichever image is inserted
 * first ends up first. Awaiting them in order is what keeps a multi-image paste
 * in the order the user pasted rather than the order the network happened to
 * finish.
 */
async function uploadBatch(
  view: EditorView,
  files: { file: File; alt: string }[],
  pos: number,
  options: ImageUploadPluginOptions
): Promise<void> {
  if (!files.length) return;
  options.onUploadStart?.();
  try {
    const handles = files.map(({ file, alt }) => startUpload(view, file, alt, pos, options));
    for (const handle of handles) {
      finishUpload(view, handle, await handle.settled, options);
    }
  } finally {
    options.onUploadEnd?.();
  }
}

function imageFilesFrom(list: FileList | null | undefined): File[] {
  const out: File[] = [];
  for (let i = 0; i < (list?.length ?? 0); i++) {
    const file = list![i];
    if (file.type.startsWith('image/')) out.push(file);
  }
  return out;
}

/** dataURL `<img>` tags in pasted HTML, e.g. a copied rich-text region. */
function dataURLImagesFrom(html: string): { src: string; alt: string }[] {
  const doc = new DOMParser().parseFromString(html, 'text/html');
  const out: { src: string; alt: string }[] = [];
  doc.querySelectorAll('img').forEach((img) => {
    if (isDataURL(img.src)) out.push({ src: img.src, alt: img.alt || 'pasted-image' });
  });
  return out;
}

/**
 * Convert pasted dataURL images to files, dropping any that are not base64.
 * Runs synchronously inside handlePaste, so it must not throw: see the note on
 * `dataURLToFile`. Anything dropped here falls through to the default paste,
 * where `transformPasted` strips it before it can reach the document.
 */
function toFiles(dataURLImages: { src: string; alt: string }[]): { file: File; alt: string }[] {
  const out: { file: File; alt: string }[] = [];
  for (const { src, alt } of dataURLImages) {
    const mime = src.match(/data:([^;]+)[;,]/)?.[1] ?? 'image/png';
    const file = dataURLToFile(src, generateImageFilename(mime));
    if (file) out.push({ file, alt });
  }
  return out;
}

/** Strip dataURL image nodes out of a slice so base64 can never reach the document. */
function stripDataURLImages(slice: Slice, state: EditorState): Slice {
  const imageType = state.schema.nodes.image;
  if (!imageType) return slice;

  let found = false;
  const scrub = (fragment: Fragment): Fragment => {
    const kept: PMNode[] = [];
    fragment.forEach((node) => {
      if (node.type === imageType && isDataURL(node.attrs.src)) {
        found = true;
        return;
      }
      kept.push(node.childCount ? node.copy(scrub(node.content)) : node);
    });
    return Fragment.fromArray(kept);
  };

  const content = scrub(slice.content);
  return found ? new Slice(content, slice.openStart, slice.openEnd) : slice;
}

export function createImageUploadPlugin(options: ImageUploadPluginOptions): Plugin {
  return new Plugin<ImageUploadPluginState>({
    key: imageUploadPluginKey,

    state: {
      init(): ImageUploadPluginState {
        return { pending: [], decos: DecorationSet.empty };
      },

      // The 4th argument is the new EditorState, which is where the y-sync
      // binding is read from. Map or rebuild FIRST, then apply the meta: the
      // completion transaction inserts the image and removes the placeholder
      // together, so the insert step must be mapped through before the removal.
      apply(tr, value, _oldState, newState): ImageUploadPluginState {
        const meta = tr.getMeta(imageUploadPluginKey) as ImageUploadMeta | undefined;
        const ystate = ySyncPluginKey.getState(newState);
        const fromYSync = tr.getMeta(ySyncPluginKey) !== undefined;

        let pending = value.pending;
        let decos = value.decos;

        if (pending.length && fromYSync && ystate?.binding) {
          // Whole-document replace: tr.mapping reports every interior position
          // as deleted, so re-derive from the CRDT anchor instead.
          const kept: PendingUpload[] = [];
          const widgets: Decoration[] = [];
          for (const p of pending) {
            const pos = p.relPos
              ? relativePositionToAbsolutePosition(
                  ystate.doc,
                  ystate.type,
                  p.relPos,
                  ystate.binding.mapping
                )
              : null;
            if (pos === null) continue;
            kept.push(p);
            widgets.push(placeholderWidget(Math.min(pos, tr.doc.content.size), p));
          }
          pending = kept;
          decos = DecorationSet.create(tr.doc, widgets);
        } else if (tr.docChanged) {
          decos = decos.map(tr.mapping, tr.doc);
          if (pending.length) {
            // A pending entry whose widget did not survive the map is
            // unrecoverable, so drop it rather than leaking the entry.
            const live = new Set(decos.find().map((d) => d.spec.uploadId as string));
            pending = pending.filter((p) => live.has(p.id));
          }
        }

        if (meta?.kind === 'add') {
          const entry: PendingUpload = { id: meta.id, label: meta.label, relPos: meta.relPos };
          pending = [...pending, entry];
          decos = decos.add(tr.doc, [placeholderWidget(meta.pos, entry)]);
        } else if (meta?.kind === 'remove') {
          const { id } = meta;
          pending = pending.filter((p) => p.id !== id);
          const gone = decos.find(undefined, undefined, (spec) => spec.uploadId === id);
          if (gone.length) decos = decos.remove(gone);
        }

        return { pending, decos };
      },
    },

    props: {
      decorations(state) {
        return imageUploadPluginKey.getState(state)?.decos ?? DecorationSet.empty;
      },

      handlePaste(view: EditorView, event: ClipboardEvent): boolean {
        const clipboardData = event.clipboardData;
        if (!clipboardData) return false;

        const imageFiles = imageFilesFrom(clipboardData.files);
        const html = imageFiles.length ? '' : clipboardData.getData('text/html');
        const batch = imageFiles.length
          ? imageFiles.map((file) => ({ file, alt: file.name }))
          : html.includes('data:image')
            ? toFiles(dataURLImagesFrom(html))
            : [];

        if (!batch.length) return false;

        // Positions must be read synchronously, before any await.
        event.preventDefault();
        void uploadBatch(view, batch, view.state.selection.from, options);
        return true;
      },

      handleDrop(view: EditorView, event: DragEvent, _slice: Slice, _moved: boolean): boolean {
        // An internal drag is a MOVE, never an upload. ProseMirror stores the
        // dragged slice in `view.dragging` for drags it initiated and its
        // default drop handling relocates the node; Chromium additionally
        // lists a dragged <img> in `dataTransfer.files`, which without this
        // guard made a simple reposition re-upload the image as a new
        // server-side copy at the drop point while leaving the original.
        if (view.dragging) return false;
        const imageFiles = imageFilesFrom(event.dataTransfer?.files);
        if (!imageFiles.length) return false;

        const at = view.posAtCoords({ left: event.clientX, top: event.clientY });
        if (!at) return false;

        event.preventDefault();
        void uploadBatch(
          view,
          imageFiles.map((file) => ({ file, alt: file.name })),
          at.pos,
          options
        );
        return true;
      },

      // Safety net for a mixed paste that reaches the default handler: strip
      // dataURL images so base64 can never enter the Yjs document. handlePaste
      // uploads them properly when it recognises the paste.
      transformPasted(slice: Slice, view: EditorView): Slice {
        return stripDataURLImages(slice, view.state);
      },
    },
  });
}

export default createImageUploadPlugin;
