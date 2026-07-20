import { watch, onUnmounted, type Ref } from 'vue';

/**
 * While `active`, publish the on-screen keyboard height to one CSS var:
 *
 *   --keyboard-height = max(0, innerHeight - visualViewport.height - offsetTop)
 *
 * A bottom-docked input translates up by it and the results scroll area pads
 * its bottom by it, so ONLY the input rides the keyboard (one compositor
 * transform) and nothing resizes. iOS/WKWebView never resizes the layout
 * viewport for the keyboard (dvh/`interactive-widget` don't track it), so
 * visualViewport is the only signal; `innerHeight` is the keyboard-invariant
 * layout viewport, `vv.height` the visual one, and `offsetTop` covers the pan.
 */
export function useVisualViewport(active: Ref<boolean>) {
  const root = document.documentElement;
  let bound = false;
  let raf = 0;

  const apply = () => {
    raf = 0;
    const vv = window.visualViewport;
    if (!vv) return;
    let kb = Math.round(window.innerHeight - vv.height - vv.offsetTop);
    // Clamp jitter and the iOS 26 bug where the inset doesn't fully reset on
    // dismiss (~24px residual, WebKit #297779).
    if (kb < 24) kb = 0;
    root.style.setProperty('--keyboard-height', `${kb}px`);
  };

  // visualViewport events fire many times per keyboard animation; coalesce to
  // one write per frame so the input rides it smoothly.
  const update = () => {
    if (raf) return;
    raf = requestAnimationFrame(apply);
  };

  const bind = () => {
    const vv = window.visualViewport;
    if (bound || !vv) return;
    vv.addEventListener('resize', update);
    vv.addEventListener('scroll', update);
    apply();
    bound = true;
  };

  const unbind = () => {
    const vv = window.visualViewport;
    if (!bound) return;
    if (raf) {
      cancelAnimationFrame(raf);
      raf = 0;
    }
    vv?.removeEventListener('resize', update);
    vv?.removeEventListener('scroll', update);
    root.style.removeProperty('--keyboard-height');
    bound = false;
  };

  watch(active, (on) => (on ? bind() : unbind()), { immediate: true });
  onUnmounted(unbind);
}

export default useVisualViewport;
