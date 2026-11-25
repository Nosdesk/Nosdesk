<!--
Render the HTML body of an inbound email inside a sandboxed iframe so
the email's CSS, layout, and any latent active content can't reach
back into the helpdesk chrome.

Why an iframe rather than `v-html` + DOMPurify alone:
  - Email HTML routinely ships its own `<style>` blocks; without
    isolation those rules leak into the surrounding ticket view.
  - `srcdoc` with `sandbox="allow-same-origin"` (no `allow-scripts`)
    blocks every flavour of script execution (inline handlers,
    `<script>` tags, `javascript:` URLs) while still letting the
    parent read `contentDocument.body.scrollHeight` so we can grow
    the iframe to match its content.
  - DOMPurify is layered on top as defence-in-depth: even with the
    sandbox we don't want tracking-pixel-ish behaviours like
    `<meta http-equiv="refresh">` slipping through.

Size and width: the email is shown at the reader's text size, and goes
down a ladder, stopping at the first step that works:

  1. As sent, zoomed by the text scale (`--text-scale`). Zoom is the only
     thing that moves type the sender wrote in px, and it keeps their
     proportions. A phone shows the body at reading size (16px, `reading`);
     elsewhere it is 14px. An email that fits stops here, untouched.
  2. Reflow. A width the sender declared wider than the frame (inline, or
     as a width attribute) gives way, and table cells may wrap
     (`body[data-reflow]`), so a 600px column reads like any other text.
     Images, video and anything declared narrower keep their widths.
  3. Shrink to fit. If something still cannot wrap (a table of whole words),
     the zoom comes down until it fits, but never past the point where the
     email's body text (the size most of its words are set in) would show
     under 12px. Small print may go under 12px with it; body text never.
  4. Pan. Past that floor the email keeps its floor size and scrolls sideways
     inside its own frame.

When step 3 left the email under the reader's size it is `fitted`, and the
reader gets one switch: "Show at my text size" (and pan) or "Fit to
width". A phone's comment puts it on the comment's head line (the parent
owns `size` and hears `fitted`); otherwise it sits under the email.
-->
<template>
  <div class="flex flex-col gap-1">
    <!--
      Sandbox flags:
        allow-same-origin                — parent reads scrollHeight for resize
        allow-popups                     — `<a target="_blank">` clicks open
        allow-popups-to-escape-sandbox   — those popups load as normal pages
                                            (without inheriting our restrictions)
      Deliberately omitted: allow-scripts, allow-forms, allow-top-navigation.
      No `<script>` block, inline handler, or `javascript:` URL can run; the
      email can't navigate the helpdesk away or submit anywhere.
    -->
    <iframe
      ref="frame"
      :srcdoc="srcdoc"
      sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
      :style="{ height: heightPx }"
      class="block w-full rounded-md border border-subtle bg-white"
      :title="t('tickets-email-html-iframe-title')"
      loading="lazy"
      referrerpolicy="no-referrer"
      @load="adjustLayout"
    />
    <div v-if="canExpand || (fitted && ownSwitch)" class="flex flex-wrap items-center gap-x-3">
      <button
        v-if="canExpand"
        type="button"
        class="text-xs text-tertiary hover:text-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-info rounded px-1 py-0.5"
        @click="expanded = !expanded"
      >
        {{ expanded ? t('tickets-email-html-show-less') : t('tickets-email-html-show-full') }}
      </button>
      <button
        v-if="fitted && ownSwitch"
        type="button"
        class="text-xs text-tertiary hover:text-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-info rounded px-1 py-0.5"
        data-email-size
        @click="localFull = !localFull"
      >
        {{ showingFull ? t('tickets-email-html-fit-width') : t('tickets-email-html-full-size') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useTextScale } from '@/composables/useTextScale'
import { fitEmailZoom } from '@/utils/textScale'
import { useFluent } from 'fluent-vue'
import DOMPurify from 'dompurify'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{
  html: string
  /**
   * True when `html` already passed the backend's sanitiser
   * (Outlook strip + ammonia). The iframe's inline CSP and the
   * sandbox attribute remain unconditional; this flag only skips
   * the client-side DOMPurify pass, which would otherwise re-run
   * for nothing on every render. Defaults to false so legacy
   * comments (pre-Pass-2 ingest) keep their belt-and-braces.
   */
  preSanitised?: boolean
  /** A phone's comment: the body at reading size (16px at the designed
   *  size) rather than 14px. */
  reading?: boolean
  /** At the reader's size and panning (`full`) or shrunk to fit (`fit`).
   *  Set by a parent that puts the switch on the comment's head line; left
   *  unset, the email keeps its own switch under the frame. A string, not a
   *  boolean, so "unset" survives Vue's boolean prop casting. */
  size?: 'full' | 'fit'

}>()

const emit = defineEmits<{
  /** Whether the email had to be held under the reader's text size. */
  (e: 'fitted', fitted: boolean): void
}>()

const frame = ref<HTMLIFrameElement | null>(null)
/** Height the email occupies after any zoom is applied. */
const measuredHeight = ref(120)
/** User toggled the iframe to its full measured height. */
const expanded = ref(false)
/** Held under the reader's text size to fit (steps 3 and 4). */
const fitted = ref(false)
/** The switch under the frame, when the parent does not own it. */
const localFull = ref(false)
const ownSwitch = computed(() => props.size === undefined)
const showingFull = computed(() => (props.size ? props.size === 'full' : localFull.value))
const textScale = useTextScale()
/** Two observers: one tracks the iframe element so we re-fit on
 *  column-width changes; one tracks the body so lazy images / web
 *  fonts settling re-trigger the height read. */
let frameObserver: ResizeObserver | null = null
let bodyObserver: ResizeObserver | null = null

/**
 * Cap the default render so a long quoted thread doesn't dominate the
 * timeline. The "Show full email" button removes the cap. 480px is
 * roughly two viewports of email body at default font size.
 */
const COLLAPSED_HEIGHT_PX = 480

const canExpand = computed(() => measuredHeight.value > COLLAPSED_HEIGHT_PX)
const heightPx = computed(() => {
  if (expanded.value || !canExpand.value) return `${measuredHeight.value}px`
  return `${COLLAPSED_HEIGHT_PX}px`
})

/**
 * Build the document the iframe renders. Sanitization runs first so
 * even with the sandbox we strip `<script>` / event handlers / known-
 * bad URI schemes; the iframe shell then wraps the result in a small
 * CSS reset so plain `<p>`s look like prose without inheriting the
 * email's chrome assumptions.
 *
 * The `:where()` wrappers on the width caps keep specificity at zero
 * so an email's own deliberate inline `style="max-width: ..."` always
 * wins. The `!important` exists to defeat HTML `width`/`height`
 * attributes (`<img width="600">`), which in CSS terms have lower
 * priority than even a zero-specificity selector but become attribute-
 * mapped presentational hints that survive a plain rule.
 */
const srcdoc = computed(() => {
  // Pre-sanitised HTML comes from the backend's Outlook strip +
  // ammonia pass and is render-ready. For legacy rows ingested
  // before Pass 2's sanitiser landed, DOMPurify is the only safety
  // layer in front of the iframe — keep running it. Either way the
  // sandbox attribute and inline CSP catch what the sanitiser
  // misses.
  const safeBody = props.preSanitised
    ? props.html
    : DOMPurify.sanitize(props.html, {
        USE_PROFILES: { html: true },
        // Belt and braces — DOMPurify already strips these by default but
        // listing them documents intent for future maintainers.
        FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'meta', 'base'],
        FORBID_ATTR: ['onerror', 'onload', 'onclick', 'onmouseover', 'onmouseenter'],
        // Keep `<style>` tags so an email's own typography survives —
        // they're scoped to the iframe document and can't leak out.
        ADD_TAGS: ['style'],
      })
  return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<!--
  Inline CSP inside srcdoc — defence-in-depth on top of the iframe
  sandbox and the backend ammonia pass, matching the Close.com
  email-rendering recipe:
    - default-src 'none'  everything not explicitly allowed (fetch,
                          ws, worker, etc.) is denied
    - script-src 'none'   blocks every flavour of script even if
                          the sandbox attribute is ever loosened
    - img-src             allows 'self' (the image proxy at
                          /api/image-proxy/... rewrites every
                          remote img to a same-origin URL during
                          Pass 3 sanitisation), data: (rarely
                          used but cheap), and cid: (inline
                          attachments). Remote http(s): is NOT
                          allowed — anything that reaches this
                          CSP without being proxied was either
                          missed by the sanitiser or injected
                          past it, and we'd rather fail the load
                          than fetch an un-vetted upstream.
    - style-src           emails ship their own <style> blocks; we
                          allow inline so typography renders, but
                          no remote stylesheet loads
    - font-src            web fonts (some marketing emails ship
                          their own): http(s), data:, same-origin
-->
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'none'; img-src 'self' data: cid:; style-src 'unsafe-inline'; font-src 'self' data: https:">
<!-- Subresource fetches (images, link clicks) inherit this policy
     from the iframe document; the parent iframe's referrerpolicy
     only governs the iframe's own load, not what runs inside. -->
<meta name="referrer" content="no-referrer">
<!-- Open every link in a new tab. -->
<base target="_blank">
<style>
  /*
   * Force light rendering inside the iframe regardless of the host
   * helpdesk theme. Newsletter emails routinely ship white-background
   * imagery, white logos on transparent backgrounds, and light-mode
   * inline styles assuming the rendering surface is white. Inverting
   * any of that for a dark-themed agent UI would flashbang the agent;
   * keeping the iframe forced-light is the same call Gmail, Front,
   * and Help Scout make. A V1.1 "invert for accessibility" toggle is
   * the natural follow-up, opt-in per-agent.
   */
  :root { color-scheme: light; }
  html, body { margin: 0; padding: 0; background: #fff; }
  body {
    padding: 6px 10px;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, system-ui, sans-serif;
    font-size: 14px;
    line-height: 1.55;
    color: #1f2937;
    /* Holds floats, so the body's box is the email's height. */
    display: flow-root;
    /* Breaks only a word longer than its line, so a table of whole words
       overflows and the ladder shrinks it instead of stacking letters. */
    overflow-wrap: break-word;
    /* zoom is set from the parent when the email natural width
       exceeds the iframe; reset on toggle / new content. */
    transform-origin: top left;
  }
  /* Cap embedded content to the iframe width. !important defeats
     attribute-mapped presentational widths from <img width="600">;
     the :where() wrapper keeps specificity at zero so the email own
     inline styles still win. */
  body :where(img, video, iframe, object, embed) {
    max-width: 100% !important;
    height: auto;
  }
  body :where(table) { max-width: 100% !important; }
  body :where(table[width]) { width: auto !important; }
  /* Step 2, only once the email overflows: a width the sender fixed is a
     wish, not a floor, and a cell may wrap. */
  body[data-reflow] :where(table, tbody, tr, td, th, div, p, center, section) {
    max-width: 100% !important;
    min-width: 0 !important;
    box-sizing: border-box;
  }
  body[data-reflow] :where([data-reflow-width]) { width: auto !important; }
  body[data-reflow] :where(td, th) { white-space: normal !important; }
  a { color: #2563eb; text-decoration: underline; }
  blockquote {
    margin: 0.5em 0;
    padding-left: 12px;
    border-left: 3px solid #d1d5db;
    color: #6b7280;
  }
  pre, code, kbd, samp {
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
    font-size: 13px;
  }
  pre { background: #f3f4f6; padding: 8px 10px; border-radius: 4px; overflow-x: auto; }
  table { border-collapse: collapse; }
  td, th { padding: 4px 8px; vertical-align: top; }
  hr { border: 0; border-top: 1px solid #e5e7eb; margin: 12px 0; }
</style>
</head>
<body>${safeBody}</body>
</html>`
})

/**
 * Lay the email out down the ladder in the header comment, then set the
 * iframe's height to its content's. Bumps round up by 8px so sub-pixel
 * layout doesn't clip the bottom line.
 *
 * `allow-same-origin` (without `allow-scripts`) is what makes the
 * `contentDocument` access here legal: the parent can read the
 * iframe's DOM, but no script inside the iframe ever runs.
 */
function adjustLayout() {
  const f = frame.value
  if (!f) return
  const doc = f.contentDocument
  const docBody = doc?.body
  const root = doc?.documentElement
  if (!docBody || !root) return
  // A hidden copy (the other breakpoint's layout) has nothing to fit yet;
  // its frame observer lays it out once it is shown.
  if (f.clientWidth === 0) {
    attachObservers(f, docBody)
    return
  }

  docBody.style.fontSize = props.reading ? '16px' : ''
  root.style.overflowX = 'hidden'
  delete docBody.dataset.reflow
  for (const el of docBody.querySelectorAll('[data-reflow-width]')) el.removeAttribute('data-reflow-width')
  setBodyZoom(docBody, 1)
  const bodyPx = bodyTextPx(docBody)

  // Tolerance: sub-pixel rounding can leave scrollWidth a hair over
  // clientWidth on a body that fits.
  const overflows = () => root.scrollWidth - root.clientWidth > 4
  const overflow = () => root.scrollWidth / root.clientWidth

  // 1. As sent, at the reader's size.
  let zoom = textScale.value
  setBodyZoom(docBody, zoom)
  let held = false
  if (overflows()) {
    // 2. Reflow: only a declared width wider than the frame gives way.
    markWideWidths(docBody, root.clientWidth / zoom)
    docBody.dataset.reflow = ''
    if (overflows()) {
      // 3. Shrink to fit, down to the floor; 4. pan past it. Only a real
      // shrink gives the reader a choice: at the floor already, fitted and
      // full are the same, so the email pans with no switch.
      const fit = fitEmailZoom(zoom, overflow(), bodyPx)
      held = fit < zoom - 0.005
      if (held && !showingFull.value) {
        zoom = fit
        setBodyZoom(docBody, zoom)
      }
      if (overflows()) root.style.overflowX = 'auto'
    }
  }
  if (held !== fitted.value) {
    fitted.value = held
    emit('fitted', held)
  }

  // The body's own box, which zoom scales in every engine. The document's
  // scrollHeight is at least the frame's, so it could never shrink.
  const pans = root.style.overflowX === 'auto'
  const next = Math.ceil(docBody.getBoundingClientRect().height) + 8 + (pans ? 12 : 0)
  if (next > 0 && next !== measuredHeight.value) measuredHeight.value = next

  attachObservers(f, docBody)
}

/** How many text runs the body size is read from; a long thread stops there. */
const TEXT_SAMPLE = 600

/**
 * The email's body text size, in px at zoom 1: the size most of its
 * characters are set in, read from up to `TEXT_SAMPLE` runs of text. Runs
 * under 6px are the tricks mail uses to hide a preheader and are left out.
 */
function bodyTextPx(body: HTMLElement): number {
  const view = body.ownerDocument.defaultView
  if (!view) return 14
  const weights = new Map<number, number>()
  const sizes = new Map<Element, number>()
  const walker = body.ownerDocument.createTreeWalker(body, NodeFilter.SHOW_TEXT)
  for (let n = 0, node = walker.nextNode(); node && n < TEXT_SAMPLE; node = walker.nextNode()) {
    const text = node.textContent?.trim().length ?? 0
    const el = node.parentElement
    if (!text || !el) continue
    n++
    let px = sizes.get(el)
    if (px === undefined) {
      px = parseFloat(view.getComputedStyle(el).fontSize)
      sizes.set(el, px)
    }
    if (px >= 6) weights.set(px, (weights.get(px) ?? 0) + text)
  }
  let best = 14
  let most = 0
  for (const [px, weight] of weights) {
    if (weight > most) {
      best = px
      most = weight
    }
  }
  return best
}

/** Mark the elements (images and video aside) whose declared width, inline or
 *  as an attribute, is wider than the frame: the reflow step frees those. */
function markWideWidths(body: HTMLElement, frameWidth: number): void {
  for (const el of body.querySelectorAll<HTMLElement>('[style*="width"], [width]')) {
    if (el instanceof HTMLImageElement || el instanceof HTMLVideoElement) continue
    const inline = /^([\d.]+)px$/.exec(el.style.width)
    const attribute = /^\d+$/.test(el.getAttribute('width') ?? '') ? Number(el.getAttribute('width')) : NaN
    const declared = inline ? Number(inline[1]) : attribute
    if (declared > frameWidth) el.setAttribute('data-reflow-width', '')
  }
}

/**
 * Apply a layout-affecting shrink. Browsers that ship `zoom` (Chromium,
 * Safari, Firefox 126+) get the simple path — `zoom` rescales the
 * layout box itself, so `scrollWidth` reflects the shrunk size and the
 * iframe height we read back is already correct. Older Firefox falls
 * back to a `transform` plus a width compensation so the layout box
 * still ends up the size we want.
 */
function setBodyZoom(body: HTMLElement, scale: number) {
  if (scale === 1) {
    body.style.removeProperty('zoom')
    body.style.removeProperty('transform')
    body.style.removeProperty('width')
    return
  }
  if (CSS && typeof CSS.supports === 'function' && CSS.supports('zoom: 0.5')) {
    body.style.zoom = String(scale)
  } else {
    body.style.transform = `scale(${scale})`
    body.style.width = `${100 / scale}%`
  }
}

/** One layout per frame, however many observers fire: a layout changes the
 *  body's size, and its own observer must not ask for another at once. */
let pending = 0
function scheduleLayout() {
  if (pending) return
  pending = requestAnimationFrame(() => {
    pending = 0
    adjustLayout()
  })
}

function attachObservers(f: HTMLIFrameElement, body: HTMLElement) {
  if (typeof ResizeObserver === 'undefined') return
  if (!frameObserver) {
    frameObserver = new ResizeObserver(scheduleLayout)
    frameObserver.observe(f)
  }
  if (!bodyObserver) {
    bodyObserver = new ResizeObserver(scheduleLayout)
    bodyObserver.observe(body)
  }
}

watch(
  () => props.html,
  () => {
    expanded.value = false
    localFull.value = false
    measuredHeight.value = 120
    frameObserver?.disconnect()
    bodyObserver?.disconnect()
    frameObserver = null
    bodyObserver = null
  },
)

// The reader's size, the reading size and the switch all
// change the layout; the height cap does not.
watch([textScale, showingFull, () => props.reading], () => adjustLayout(), { flush: 'post' })

onBeforeUnmount(() => {
  if (pending) cancelAnimationFrame(pending)
  frameObserver?.disconnect()
  bodyObserver?.disconnect()
  frameObserver = null
  bodyObserver = null
})
</script>
