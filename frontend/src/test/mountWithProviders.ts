/**
 * Component-test harness: mounts a component under the same providers the
 * app installs in `App.vue` and `main.ts`, so a primitive that reads the
 * fluent plugin, Reka's ConfigProvider or the TooltipProvider renders as it
 * does in the app. Translations resolve to the message id (no catalogue is
 * loaded), which keeps assertions readable and locale-free.
 */
import { defineComponent, h, type Component } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import { ConfigProvider, TooltipProvider } from 'reka-ui'

// Stores read `localStorage` at setup. Newer Node ships its own global that
// is undefined unless a storage file is configured and shadows jsdom's, so
// back it with an in-memory Storage when it is not usable.
if (typeof globalThis.localStorage?.getItem !== 'function') {
  const store = new Map<string, string>()
  const memoryStorage: Storage = {
    get length() {
      return store.size
    },
    clear: () => store.clear(),
    getItem: (k) => store.get(k) ?? null,
    key: (i) => [...store.keys()][i] ?? null,
    removeItem: (k) => void store.delete(k),
    setItem: (k, v) => void store.set(k, String(v)),
  }
  Object.defineProperty(globalThis, 'localStorage', { value: memoryStorage, configurable: true })
}

// jsdom has no matchMedia; stores that track the system theme need one.
// Nothing matches by default, and specs that drive a breakpoint install
// their own mock per test.
if (typeof window.matchMedia !== 'function') {
  window.matchMedia = (query: string): MediaQueryList => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })
}

// jsdom has no ResizeObserver; Reka's Slider measures its thumb with one.
// Nothing resizes in a test, so a stub that never calls back is enough.
if (typeof globalThis.ResizeObserver !== 'function') {
  class StubResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  Object.defineProperty(globalThis, 'ResizeObserver', { value: StubResizeObserver, configurable: true })
}

// jsdom has no scrollIntoView; listboxes call it on the highlighted row.
if (typeof Element.prototype.scrollIntoView !== 'function') {
  Element.prototype.scrollIntoView = () => {}
}

export function mountWithProviders(
  component: Component,
  props: Record<string, unknown> = {},
  slots: Record<string, () => unknown> = {},
): VueWrapper {
  const fluent = createFluentVue({ bundles: [new FluentBundle('en-US')] })
  // The host forwards its attrs on top of the initial props, so
  // `wrapper.setProps` reaches the component under test.
  const Host = defineComponent({
    inheritAttrs: false,
    setup(_, { attrs }) {
      return () =>
        h(ConfigProvider, {}, () =>
          h(TooltipProvider, { delayDuration: 0 }, () => h(component, { ...props, ...attrs }, slots)),
        )
    },
  })
  return mount(Host, { attachTo: document.body, global: { plugins: [fluent, createPinia()] } })
}
