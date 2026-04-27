/**
 * `v-prefetch="'/route'"` directive.
 *
 * Warms the lazy chunk + Pinia Colada caches for a target route
 * before the user actually navigates. Triggers fire on:
 *   - first viewport intersection (gives mobile users prefetch
 *     without requiring hover, which doesn't exist on touch)
 *   - mouse pointer entering the element (desktop hover intent)
 *   - keyboard focus (a11y-friendly intent signal)
 *
 * Once prefetched, listeners detach. The directive is a no-op
 * on subsequent triggers.
 *
 * What gets prefetched:
 *   1. The route component's lazy import. The chunk download
 *      starts immediately and sits in the browser cache, so the
 *      eventual click-to-render is bound by network only for
 *      data, not bundle.
 *   2. Any `meta.loaders` on the matched route(s). Pinia Colada
 *      data loaders run their `query` function and prime the
 *      cache. By the time the user navigates, both chunk and
 *      data are ready.
 *
 * Best applied to navigation links in the app shell. Skip for
 * one-off in-content links: the cost (one extra fetch per
 * hovered link) outweighs the benefit when a link won't be
 * clicked.
 */
import type { DirectiveBinding, ObjectDirective } from 'vue'
import type { Router, RouteLocationRaw } from 'vue-router'

interface PrefetchState {
  cleanup: () => void
  done: boolean
}

const STATE_KEY = '__nosdeskPrefetch__' as const

interface PrefetchableElement extends HTMLElement {
  [STATE_KEY]?: PrefetchState
}

function resolveRouter(el: HTMLElement): Router | undefined {
  // Vue Router stores the router instance on the app's
  // `_context.config.globalProperties.$router`. We walk up the
  // DOM to the Vue app root to find it. This avoids requiring
  // the directive to be installed with a closure-bound router.
  let cursor: (Node & { __vue_app__?: { config?: { globalProperties?: { $router?: Router } } } }) | null = el
  while (cursor) {
    const app = (cursor as { __vue_app__?: { config?: { globalProperties?: { $router?: Router } } } }).__vue_app__
    if (app?.config?.globalProperties?.$router) {
      return app.config.globalProperties.$router
    }
    cursor = cursor.parentNode
  }
  return undefined
}

async function prefetch(el: HTMLElement, to: RouteLocationRaw) {
  const router = resolveRouter(el)
  if (!router) return
  let resolved
  try {
    resolved = router.resolve(to)
  } catch {
    return
  }
  // Fire chunk import + loader queries in parallel. We don't
  // await; the directive is fire-and-forget.
  for (const record of resolved.matched) {
    // Chunk: the `components` map holds either eager components
    // or lazy import functions. Calling the function returns a
    // Promise the browser will cache.
    const componentEntries = record.components ?? {}
    for (const value of Object.values(componentEntries)) {
      if (typeof value === 'function') {
        try {
          ;(value as () => Promise<unknown>)()
        } catch {
          // Swallow: a failed prefetch isn't a user-visible
          // error. The eventual real navigation will surface it.
        }
      }
    }
    // Loaders: each is a `defineColadaLoader` composable. Call
    // them to prime the cache. The "no current instance"
    // warnings Vue normally fires for composable-outside-setup
    // are intentionally suppressed by Pinia Colada's
    // injectable-context support.
    const loaders = (record.meta as { loaders?: Array<() => unknown> }).loaders
    if (Array.isArray(loaders)) {
      for (const loader of loaders) {
        try {
          loader()
        } catch {
          // Same as above: fire-and-forget.
        }
      }
    }
  }
}

function setupTriggers(el: PrefetchableElement, to: RouteLocationRaw) {
  if (el[STATE_KEY]) return

  let observer: IntersectionObserver | null = null

  const fire = () => {
    if (el[STATE_KEY]?.done) return
    if (el[STATE_KEY]) el[STATE_KEY].done = true
    cleanup()
    prefetch(el, to)
  }

  function cleanup() {
    el.removeEventListener('pointerenter', fire)
    el.removeEventListener('focusin', fire)
    observer?.disconnect()
    observer = null
  }

  el.addEventListener('pointerenter', fire, { once: true })
  el.addEventListener('focusin', fire, { once: true })

  if (typeof IntersectionObserver !== 'undefined') {
    observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            fire()
            break
          }
        }
      },
      // Prefetch slightly before the link enters the viewport,
      // so by the time the user can see and click it, the
      // chunk + data are already in flight.
      { rootMargin: '200px' },
    )
    observer.observe(el)
  }

  el[STATE_KEY] = {
    cleanup,
    done: false,
  }
}

export const vPrefetch: ObjectDirective<PrefetchableElement, RouteLocationRaw> = {
  mounted(el, binding: DirectiveBinding<RouteLocationRaw>) {
    if (!binding.value) return
    setupTriggers(el, binding.value)
  },
  updated(el, binding: DirectiveBinding<RouteLocationRaw>) {
    // If the target changes, tear down and re-arm. Rare in
    // practice (most prefetch targets are static), but keeps
    // the directive consistent with Vue update semantics.
    if (el[STATE_KEY]) {
      el[STATE_KEY]?.cleanup()
      el[STATE_KEY] = undefined
    }
    if (!binding.value) return
    setupTriggers(el, binding.value)
  },
  beforeUnmount(el) {
    el[STATE_KEY]?.cleanup()
    el[STATE_KEY] = undefined
  },
}
