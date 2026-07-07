import { watch, onUnmounted, type Ref } from 'vue';

/**
 * Mirror the visual viewport into root CSS custom properties so
 * fixed overlays can size themselves to the *visible* area rather
 * than the layout viewport:
 *
 * - `--visual-viewport-height`: the visible height in px.
 * - `--keyboard-inset`: how much of the layout viewport's bottom is
 *   obscured (on-screen keyboard, mostly), in px.
 *
 * Why this exists: neither WKWebView (the Tauri mobile shell) nor
 * iOS Safari resizes the layout viewport when the on-screen keyboard
 * opens, so `100dvh` still measures the full screen and anything
 * bottom-anchored disappears behind the keyboard. The visualViewport
 * API is the only web-side signal that tracks it (`interactive-
 * widget=resizes-content` in the viewport meta covers Chrome/Android
 * by resizing the layout viewport instead — there the inset computes
 * to ~0 and the vars are harmlessly redundant).
 *
 * Listeners are bound only while `active` is true — an overlay passes
 * its open state — so the app pays nothing at idle. Consumers style
 * with `var(--visual-viewport-height, 100dvh)`: the fallback keeps
 * every non-listening moment (and browsers without the API) on plain
 * dvh behaviour.
 */
export function useVisualViewport(active: Ref<boolean>) {
  const root = document.documentElement;
  let bound = false;

  const update = () => {
    const vv = window.visualViewport;
    if (!vv) return;
    // offsetTop accounts for the visible region being pushed down
    // (pinch-zoom pan, or WKWebView shifting to reveal an input);
    // subtracting it keeps the inset an honest "hidden at the
    // bottom" measure.
    const inset = Math.max(0, window.innerHeight - vv.height - vv.offsetTop);
    root.style.setProperty('--visual-viewport-height', `${Math.round(vv.height)}px`);
    root.style.setProperty('--keyboard-inset', `${Math.round(inset)}px`);
  };

  const bind = () => {
    const vv = window.visualViewport;
    if (bound || !vv) return;
    vv.addEventListener('resize', update);
    vv.addEventListener('scroll', update);
    update();
    bound = true;
  };

  const unbind = () => {
    const vv = window.visualViewport;
    if (!bound) return;
    vv?.removeEventListener('resize', update);
    vv?.removeEventListener('scroll', update);
    // Drop the vars so consumers fall back to their dvh defaults
    // instead of a stale snapshot from the last open.
    root.style.removeProperty('--visual-viewport-height');
    root.style.removeProperty('--keyboard-inset');
    bound = false;
  };

  watch(active, (on) => (on ? bind() : unbind()), { immediate: true });
  onUnmounted(unbind);
}

export default useVisualViewport;
