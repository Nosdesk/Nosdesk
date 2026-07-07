/*
 * Launch-splash controller. Loaded render-blocking from <head> (before
 * first paint), so it sets the splash colours + runtime marker before
 * the #app-splash element paints, then tears the splash down once the
 * app's first screen is up.
 *
 * Why a separate file (not inline in index.html): the app CSP is
 * `script-src 'self' 'unsafe-eval'` with no 'unsafe-inline'/nonce, so
 * inline scripts are blocked. An origin-served file is 'self'-allowed.
 * The splash visuals/animation are inline <style> (CSP allows that).
 *
 * Contract with the rest of the app:
 *   - `main.ts` sets `data-ready` on #app-splash after the first real
 *     screen has painted (double-rAF post-mount).
 *   - the resolved theme is cached in localStorage `nosdesk_launch_theme`
 *     by the theme store, so the field + N match the user's theme.
 */
(function () {
  var MIN_DISPLAY_MS = 700 // app: keep the intro up long enough for a sheen pass
  var SAFETY_MS = 5000 // never trap the splash if `data-ready` never comes
  var FADE_MS = 360 // covers the crossfade exit (300ms) + buffer

  var root = document.documentElement

  // --- Colours (before first paint) --------------------------------------
  var bg = null
  var fg = null
  try {
    var cached = JSON.parse(localStorage.getItem('nosdesk_launch_theme') || '{}')
    if (cached && typeof cached.app === 'string') bg = cached.app
    if (cached && typeof cached.accent === 'string') fg = cached.accent
  } catch (e) {
    /* private mode / malformed: fall through to defaults */
  }
  // First launch (no cache): match OS appearance so a light-mode user
  // never gets a dark flash; dark brand is the default field.
  var prefersDark = !window.matchMedia || window.matchMedia('(prefers-color-scheme: dark)').matches
  if (!bg) bg = prefersDark ? '#08090a' : '#f3f4f6'
  if (!fg) fg = '#FF6B1A'
  root.style.setProperty('--splash-bg', bg)
  root.style.setProperty('--splash-fg', fg)

  // Runtime marker gates the animated N to the mobile app (set before
  // paint so plain web never even flashes the mark). `window.isTauri`
  // is injected by Tauri before the page loads.
  var isApp = !!window.isTauri
  root.classList.add(isApp ? 'runtime-app' : 'runtime-web')

  // --- Teardown ----------------------------------------------------------
  function whenReady(el, done, minMs) {
    var startT = Date.now()
    var fired = false
    function fire() {
      if (fired) return
      fired = true
      setTimeout(done, Math.max(0, minMs - (Date.now() - startT)))
    }
    if (el.hasAttribute('data-ready')) {
      fire()
      return
    }
    var obs = new MutationObserver(function () {
      if (el.hasAttribute('data-ready')) {
        obs.disconnect()
        fire()
      }
    })
    obs.observe(el, { attributes: true, attributeFilter: ['data-ready'] })
    // Safety net: a hung boot must not trap the splash.
    setTimeout(function () {
      obs.disconnect()
      fire()
    }, SAFETY_MS)
  }

  function exit(el) {
    el.setAttribute('data-exit', '')
    // A CSS animation drives the longest part of the exit, so a plain
    // timer (not transitionend, which fires on the shorter bg fade) is
    // the deterministic removal trigger. Removing the node leaves zero
    // residual layer.
    setTimeout(function () {
      if (el.parentNode) el.parentNode.removeChild(el)
    }, FADE_MS)
  }

  function start() {
    var el = document.getElementById('app-splash')
    if (!el) return
    // App: full intro with a minimum on-screen time. Web: no branded
    // intro (the N is hidden via runtime-web) — the themed field just
    // covers the pre-mount flash and leaves as soon as the app is ready.
    whenReady(el, function () { exit(el) }, isApp ? MIN_DISPLAY_MS : 0)
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', start)
  } else {
    start()
  }
})()
