/**
 * Resizable image NodeView for the collaborative editor.
 *
 * Follows the documented ProseMirror pattern for resize handles (marijn,
 * discuss "Image resize" / "Draggable and NodeViews") and Tiptap's
 * onResize/onCommit split:
 *
 * - Handles and the adjust toolbar are NodeView-internal DOM, visible only
 *   while the node holds a NodeSelection (`selectNode`/`deselectNode`) and
 *   the view is editable.
 * - A drag mutates DOM style only; exactly ONE transaction commits the new
 *   width on pointerup (one resize = one undo step, no per-pixel churn to
 *   collaborators).
 * - `stopEvent` swallows pointer events on our controls so they never start
 *   ProseMirror's native node drag, but lets every `drag*` event through so
 *   grabbing the image body still drags the node (the schema keeps
 *   `draggable: true`).
 * - y-prosemirror applies remote edits as whole-document replaces, which can
 *   destroy this NodeView mid-drag. `getPos()` is therefore re-read at
 *   commit time (bail on undefined), and `destroy()` tears down the
 *   document-level listeners so an interrupted drag dies cleanly.
 */
import type { Node as PMNode } from 'prosemirror-model';
import { NodeSelection } from 'prosemirror-state';
import type { EditorView, NodeView } from 'prosemirror-view';

const MIN_WIDTH_PX = 60;

export interface ImageNodeViewOptions {
  /** Fluent translate, for control labels. */
  t: (key: string) => string;
}

type Align = 'left' | 'center' | 'right';

interface DragState {
  pointerId: number;
  startX: number;
  startWidth: number;
  /** -1 for west handles (drag left grows), +1 for east handles. */
  sign: 1 | -1;
  lastWidth: number;
  onMove: (e: PointerEvent) => void;
  onUp: (e: PointerEvent) => void;
}

export class ImageNodeView implements NodeView {
  dom: HTMLSpanElement;
  private img: HTMLImageElement;
  private badge: HTMLSpanElement;
  private toolbar: HTMLSpanElement;
  private node: PMNode;
  private drag: DragState | null = null;

  constructor(
    node: PMNode,
    private view: EditorView,
    private getPos: () => number | undefined,
    private options: ImageNodeViewOptions,
  ) {
    this.node = node;

    this.dom = document.createElement('span');
    this.dom.className = 'pm-image';
    this.dom.contentEditable = 'false';

    this.img = document.createElement('img');
    this.dom.appendChild(this.img);

    for (const dir of ['nw', 'ne', 'sw', 'se'] as const) {
      const handle = document.createElement('span');
      handle.className = 'pm-image__handle';
      handle.dataset.dir = dir;
      handle.addEventListener('pointerdown', (e) => this.startDrag(e, dir));
      this.dom.appendChild(handle);
    }

    this.badge = document.createElement('span');
    this.badge.className = 'pm-image__badge';
    this.dom.appendChild(this.badge);

    this.toolbar = this.buildToolbar();
    this.dom.appendChild(this.toolbar);

    this.syncAttrs(node);
  }

  private buildToolbar(): HTMLSpanElement {
    const bar = document.createElement('span');
    bar.className = 'pm-image__toolbar';

    const button = (label: string, glyph: string, onClick: () => void) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = 'pm-image__tool';
      b.title = label;
      b.setAttribute('aria-label', label);
      b.innerHTML = glyph;
      // pointerdown (not click) so focus never leaves the editor.
      b.addEventListener('pointerdown', (e) => {
        e.preventDefault();
        e.stopPropagation();
        onClick();
      });
      bar.appendChild(b);
      return b;
    };

    const alignIcon = (dir: Align) => {
      const x2 = dir === 'left' ? 'M3 12h10M3 18h14' : dir === 'right' ? 'M11 12h10M7 18h14' : 'M7 12h10M5 18h14';
      return `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M3 6h18${x2.startsWith('M') ? '' : ''}"></path><path d="${x2}"></path></svg>`;
    };

    const t = this.options.t;
    for (const dir of ['left', 'center', 'right'] as const) {
      const b = button(t(`editor-image-align-${dir}`), alignIcon(dir), () =>
        this.setAttrsCommitted({ align: this.node.attrs.align === dir ? null : dir }),
      );
      b.dataset.align = dir;
    }

    const sep = document.createElement('span');
    sep.className = 'pm-image__toolsep';
    bar.appendChild(sep);

    button(
      t('editor-image-size-half'),
      '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="10" height="14" rx="1"></rect><path d="M17 5h4v14h-4" stroke-dasharray="2 2"></path></svg>',
      () => this.setAttrsCommitted({ width: Math.round(this.containerWidth() / 2) }),
    );
    button(
      t('editor-image-size-full'),
      '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="5" width="18" height="14" rx="1"></rect></svg>',
      () => this.setAttrsCommitted({ width: this.containerWidth() }),
    );
    button(
      t('editor-image-size-reset'),
      '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 3-6.7"></path><path d="M3 4v5h5"></path></svg>',
      () => this.setAttrsCommitted({ width: null }),
    );

    return bar;
  }

  /** Usable content width of the editor, the natural "100%" for an image. */
  private containerWidth(): number {
    const editor = this.view.dom as HTMLElement;
    const styles = window.getComputedStyle(editor);
    return Math.max(
      MIN_WIDTH_PX,
      Math.round(
        editor.clientWidth - parseFloat(styles.paddingLeft) - parseFloat(styles.paddingRight),
      ),
    );
  }

  private syncAttrs(node: PMNode): void {
    const { src, alt, title, width, align } = node.attrs as {
      src: string;
      alt: string | null;
      title: string | null;
      width: number | null;
      align: Align | null;
    };
    if (this.img.getAttribute('src') !== src) this.img.setAttribute('src', src);
    if (alt) this.img.setAttribute('alt', alt);
    else this.img.removeAttribute('alt');
    if (title) this.img.setAttribute('title', title);
    else this.img.removeAttribute('title');

    // Skip style writes mid-drag: the drag owns the live width.
    if (!this.drag) {
      this.img.style.width = width != null ? `${width}px` : '';
    }
    if (align) this.dom.setAttribute('data-align', align);
    else this.dom.removeAttribute('data-align');

    for (const b of this.toolbar.querySelectorAll<HTMLButtonElement>('button[data-align]')) {
      b.classList.toggle('is-active', b.dataset.align === (align ?? ''));
    }
  }

  /** Commit an attr patch as one transaction, keeping the node selected. */
  private setAttrsCommitted(patch: Partial<{ width: number | null; align: Align | null }>): void {
    const pos = this.getPos();
    if (pos == null) return;
    const attrs = { ...this.node.attrs, ...patch };
    let tr = this.view.state.tr.setNodeMarkup(pos, undefined, attrs);
    tr = tr.setSelection(NodeSelection.create(tr.doc, pos));
    this.view.dispatch(tr);
    this.view.focus();
  }

  private startDrag(e: PointerEvent, dir: 'nw' | 'ne' | 'sw' | 'se'): void {
    if (!this.view.editable || this.drag) return;
    e.preventDefault();
    e.stopPropagation();

    const startWidth = this.img.getBoundingClientRect().width;
    const sign: 1 | -1 = dir === 'ne' || dir === 'se' ? 1 : -1;

    const onMove = (ev: PointerEvent) => {
      if (!this.drag || ev.pointerId !== this.drag.pointerId) return;
      const next = Math.round(
        Math.min(
          this.containerWidth(),
          Math.max(MIN_WIDTH_PX, this.drag.startWidth + sign * (ev.clientX - this.drag.startX)),
        ),
      );
      this.drag.lastWidth = next;
      this.img.style.width = `${next}px`;
      this.badge.textContent = `${next}px`;
      this.dom.classList.add('is-resizing');
    };

    const onUp = (ev: PointerEvent) => {
      if (!this.drag || ev.pointerId !== this.drag.pointerId) return;
      const { lastWidth, startWidth: initial } = this.drag;
      this.teardownDrag();
      if (Math.abs(lastWidth - initial) >= 1) {
        this.setAttrsCommitted({ width: lastWidth });
      } else {
        // No effective change: restore whatever the document says.
        this.syncAttrs(this.node);
      }
    };

    this.drag = {
      pointerId: e.pointerId,
      startX: e.clientX,
      startWidth,
      sign,
      lastWidth: Math.round(startWidth),
      onMove,
      onUp,
    };
    document.addEventListener('pointermove', onMove);
    document.addEventListener('pointerup', onUp);
    document.addEventListener('pointercancel', onUp);
  }

  private teardownDrag(): void {
    if (!this.drag) return;
    document.removeEventListener('pointermove', this.drag.onMove);
    document.removeEventListener('pointerup', this.drag.onUp);
    document.removeEventListener('pointercancel', this.drag.onUp);
    this.drag = null;
    this.badge.textContent = '';
    this.dom.classList.remove('is-resizing');
  }

  update(node: PMNode): boolean {
    if (node.type !== this.node.type) return false;
    this.node = node;
    this.syncAttrs(node);
    return true;
  }

  selectNode(): void {
    if (this.view.editable) this.dom.classList.add('is-selected');
  }

  deselectNode(): void {
    this.dom.classList.remove('is-selected');
  }

  stopEvent(event: Event): boolean {
    // Native node drag must keep working when grabbing the image body.
    if (/drag/.test(event.type)) return false;
    // Everything on our controls is ours; keep ProseMirror out of it.
    const target = event.target as HTMLElement | null;
    return !!target && target !== this.img && this.dom.contains(target);
  }

  ignoreMutation(record: MutationRecord | { type: 'selection' }): boolean {
    // Leaf view: every DOM mutation here is handle/badge/toolbar churn we
    // caused ourselves. Only selection reads must reach ProseMirror.
    return record.type !== 'selection';
  }

  destroy(): void {
    this.teardownDrag();
  }
}
