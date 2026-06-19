import './assets/main.css'
import './services/apiConfig' // Import axios configuration

// Initialise remote logging for debugging (can be disabled via localStorage)
import { interceptConsole } from './utils/remoteLogger'
interceptConsole()

import { createApp } from 'vue'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'
import { DataLoaderPlugin } from 'vue-router/experimental'

import App from './App.vue'
import router from './router'
import { vSafeHtml } from './directives/vSafeHtml'
import { vTwemoji } from './directives/vTwemoji'
import { vPrefetch } from './directives/vPrefetch'

const app = createApp(App)

// Register global directives
app.directive('safe-html', vSafeHtml)
app.directive('twemoji', vTwemoji)
app.directive('prefetch', vPrefetch)

const pinia = createPinia()
app.use(pinia)

// Fluent-based i18n. Must register after Pinia (the i18n module
// reads dateStore.locale for the active bundle) and before
// components render so the first paint already speaks the right
// language. Locale follows dateStore.locale, which auth.ts seeds
// from /me's effective_locale on login.
import { createI18n as createI18nPlugin } from './i18n'
app.use(createI18nPlugin(pinia))
// Pinia Colada must register AFTER Pinia. Provides the canonical
// async data layer (queries, mutations, optimistic updates,
// query cache, route loader integration). See
// `~/Documents/notes/technology/web development/loading-states-architecture.md`
// for the architectural rationale.
app.use(PiniaColada, {})
// Vue Router Data Loaders. MUST register before `app.use(router)`
// so loaders are picked up during the initial navigation. Loaders
// run during route transitions (render-as-you-fetch), not after
// component mount, so /inbox starts loading data the moment the
// user clicks the link.
app.use(DataLoaderPlugin, { router })
app.use(router)

// Initialize theme store to respect system preferences for guests
// This ensures dark mode works even when not logged in
import { useThemeStore } from './stores/theme'
useThemeStore(pinia)

// Fetch instance config (routing topology) in parallel with the initial route
// resolution so its value is settled before first paint. The fetch defaults to
// 'host' and never rejects, so it can't block or break the mount.
import { fetchInstanceConfig } from './services/instanceConfig'

Promise.all([fetchInstanceConfig(), router.isReady()]).then(() => {
  app.mount('#app')
})
