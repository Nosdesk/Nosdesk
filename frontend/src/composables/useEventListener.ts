/**
 * Auto-cleaned, optionally-conditional DOM event listener.
 *
 * Replaces the manual addEventListener / removeEventListener
 * pair pattern with a composable that:
 *   - attaches on mount (or when `when` becomes truthy)
 *   - removes on unmount (or when `when` becomes falsy)
 *   - cleans up via the active effect scope, so consumers don't
 *     leak listeners if they forget onUnmounted
 *
 * The `when` predicate gives consumers a way to gate listeners
 * by reactive state — e.g. only attach scroll/resize listeners
 * while a popover is open. Without this gate every popover on
 * the page would carry a permanent set of listeners just in
 * case it might open later.
 */
import { onScopeDispose, watch, type Ref } from 'vue'

type TargetMap = {
  Window: Window
  Document: Document
  HTMLElement: HTMLElement
}

interface Options {
  /** When provided, the listener is attached only while this
   * ref is truthy. Toggling re-attaches / re-removes. */
  when?: Ref<boolean>
  /** Standard addEventListener options. */
  capture?: boolean
  passive?: boolean
}

export function useEventListener<
  K extends keyof WindowEventMap,
>(
  target: TargetMap['Window'],
  type: K,
  handler: (event: WindowEventMap[K]) => void,
  options?: Options,
): void

export function useEventListener<
  K extends keyof DocumentEventMap,
>(
  target: TargetMap['Document'],
  type: K,
  handler: (event: DocumentEventMap[K]) => void,
  options?: Options,
): void

export function useEventListener<
  K extends keyof HTMLElementEventMap,
>(
  target: TargetMap['HTMLElement'],
  type: K,
  handler: (event: HTMLElementEventMap[K]) => void,
  options?: Options,
): void

export function useEventListener(
  target: EventTarget,
  type: string,
  handler: EventListenerOrEventListenerObject,
  options: Options = {},
): void {
  const eventOpts: AddEventListenerOptions = {
    capture: options.capture ?? false,
    passive: options.passive,
  }
  let attached = false

  function attach(): void {
    if (attached) return
    target.addEventListener(type, handler, eventOpts)
    attached = true
  }
  function detach(): void {
    if (!attached) return
    target.removeEventListener(type, handler, eventOpts)
    attached = false
  }

  if (options.when) {
    // Track the gate ref. `immediate` so the initial state is
    // honoured on mount instead of waiting for the first toggle.
    watch(
      options.when,
      (open) => {
        if (open) attach()
        else detach()
      },
      { immediate: true },
    )
  } else {
    attach()
  }

  // onScopeDispose fires for both component unmount and
  // effectScope.stop(), so the listener is cleaned up regardless
  // of how the consumer is structured.
  onScopeDispose(detach)
}
