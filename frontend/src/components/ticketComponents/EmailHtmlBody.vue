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

Width handling: marketing-style emails routinely ship 600-700px
fixed-width tables, `width="600"` images, and inline `width: 600px`
styles. The iframe's reset stylesheet caps embedded content with
`max-width: 100%`, which absorbs the common cases at zero cost.
For emails that still overflow after that — typically nested tables
with explicit pixel widths — we fall back to a CSS `zoom` shrink
so the whole layout fits the iframe rather than producing a
horizontal scrollbar. The "Show full email" button switches that
shrink off and lets the user scroll horizontally if a faithful
render is more useful than a fitting one.
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
    <div v-if="hasOverflowed || canExpand" class="flex items-center gap-3">
      <button
        v-if="canExpand"
        type="button"
        class="text-xs text-tertiary hover:text-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-status-info rounded px-1 py-0.5"
        @click="expanded = !expanded"
      >
        {{ expanded ? t('tickets-email-html-show-less') : t('tickets-email-html-show-full') }}
      </button>
      <span v-if="hasOverflowed && !expanded" class="text-2xs text-tertiary">
        {{ t('tickets-email-html-scaled', { pct: Math.round(activeScale * 100) }) }}
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
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
}>()

const frame = ref<HTMLIFrameElement | null>(null)
/** Height the email occupies after any zoom is applied. */
const measuredHeight = ref(120)
/** User toggled the iframe to its full measured height. */
const expanded = ref(false)
/** Last computed shrink factor, 0-1. `1` means no scaling needed. */
const activeScale = ref(1)
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
const hasOverflowed = computed(() => activeScale.value < 0.999)
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
    word-wrap: break-word;
    overflow-wrap: anywhere;
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
 * Read the iframe's content size and apply the right pair of layout
 * adjustments: a `zoom` shrink when the email's natural width still
 * overflows after the CSS caps, and a height set so the iframe element
 * exactly matches its body. Bumps round up by 8px so sub-pixel layout
 * doesn't clip the bottom line.
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
  if (!docBody) return

  // Reset previous zoom before re-measuring; otherwise scrollWidth
  // is reported in the *post-zoom* coordinate system and the next
  // shrink ratio is computed against an already-shrunk reference.
  setBodyZoom(docBody, 1)

  const containerWidth = f.clientWidth
  const naturalWidth = docBody.scrollWidth
  // Tolerance — sub-pixel rounding can leave scrollWidth a hair over
  // clientWidth on perfectly-fitting bodies. 4px is below the smallest
  // visible scrollbar artifact while still catching real overflow.
  const overflowing = naturalWidth - containerWidth > 4

  // Scaling is suppressed in expanded mode so users can opt for a
  // faithful render with horizontal scroll when the shrunk view is
  // harder to read than the original.
  if (overflowing && !expanded.value && containerWidth > 0) {
    const scale = Math.max(0.5, containerWidth / naturalWidth)
    setBodyZoom(docBody, scale)
    activeScale.value = scale
  } else {
    activeScale.value = 1
  }

  const next = Math.ceil(docBody.scrollHeight) + 8
  if (next > 0 && next !== measuredHeight.value) measuredHeight.value = next

  attachObservers(f, docBody)
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

function attachObservers(f: HTMLIFrameElement, body: HTMLElement) {
  if (typeof ResizeObserver === 'undefined') return
  if (!frameObserver) {
    frameObserver = new ResizeObserver(() => adjustLayout())
    frameObserver.observe(f)
  }
  if (!bodyObserver) {
    bodyObserver = new ResizeObserver(() => adjustLayout())
    bodyObserver.observe(body)
  }
}

watch(
  () => props.html,
  () => {
    expanded.value = false
    measuredHeight.value = 120
    activeScale.value = 1
    frameObserver?.disconnect()
    bodyObserver?.disconnect()
    frameObserver = null
    bodyObserver = null
  },
)

watch(expanded, () => {
  // Toggling expansion changes the policy on whether to scale; re-run
  // the layout pass so the iframe transitions cleanly.
  adjustLayout()
})

onBeforeUnmount(() => {
  frameObserver?.disconnect()
  bodyObserver?.disconnect()
  frameObserver = null
  bodyObserver = null
})
</script>
