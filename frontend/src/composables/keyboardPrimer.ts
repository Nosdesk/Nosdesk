/**
 * Raise the iOS software keyboard SYNCHRONOUSLY inside the opening tap.
 *
 * WKWebView presents the keyboard only when `focus()` runs during a live user
 * activation (the tap handler). Our real search input is teleported and only
 * mounts on the next render tick, too late to focus in the gesture, so a later
 * `focus()` moves the caret without showing the keyboard (the setTimeout bug).
 *
 * Fix: focus a persistent, near-invisible "primer" input synchronously from the
 * opener (`primeKeyboard()` at the top of openSearch). That raises the keyboard
 * now; when the real input mounts, focusing it just transfers the caret under a
 * keyboard that is already up (no fresh gesture needed to MOVE focus).
 */
let primer: HTMLInputElement | null = null;

function ensurePrimer(): HTMLInputElement {
  if (primer) return primer;
  const el = document.createElement('input');
  el.type = 'text';
  el.setAttribute('aria-hidden', 'true');
  el.tabIndex = -1;
  Object.assign(el.style, {
    position: 'fixed',
    top: '0',
    left: '0',
    width: '1px',
    height: '1px',
    // Must stay "rendered" (not display:none / visibility:hidden), and >=16px so
    // focusing it doesn't trigger iOS's zoom-on-small-input.
    fontSize: '16px',
    opacity: '0.01',
    color: 'transparent',
    caretColor: 'transparent',
    border: '0',
    padding: '0',
    background: 'transparent',
    pointerEvents: 'none',
    zIndex: '-1',
  });
  document.body.appendChild(el);
  primer = el;
  return el;
}

/** Call SYNCHRONOUSLY from the opening gesture (top of openSearch). Touch only. */
export function primeKeyboard(): void {
  if (!window.matchMedia('(pointer: coarse)').matches) return;
  try {
    ensurePrimer().focus({ preventScroll: true });
  } catch {
    // Best-effort: a failure just means the real input's focus shows the keyboard.
  }
}
