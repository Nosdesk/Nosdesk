import './assets/main.css'
import { configurePlatform } from './platform'
import { initServerGate } from './platform/serverGate'

import { interceptConsole } from './utils/remoteLogger'

import { createApp, nextTick } from 'vue'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'

import App from './App.vue'
import router from './router'
import { vSafeHtml } from './directives/vSafeHtml'
import { vTwemoji } from './directives/vTwemoji'
import { vPrefetch } from './directives/vPrefetch'
import { vScrollRestore } from './directives/vScrollRestore'
import { createI18n as createI18nPlugin } from './i18n'
import { useThemeStore } from './stores/theme'
import { fetchInstanceConfig } from '@nosdesk/core/services/instanceConfig'

async function bootstrap() {
  // Configure the @nosdesk/core seams for the current platform (web: cookies +
  // localStorage; Tauri: bearer + native HTTP) before anything uses them.
  await configurePlatform()

  // Native app: decide whether the first-run "choose your server" screen is
  // needed before the app renders (no-op on the web).
  await initServerGate()

  // Remote logging for debugging (can be disabled via localStorage).
  interceptConsole()

  const app = createApp(App)

  // Own scroll restoration (see vScrollRestore): stop the browser competing with
  // our per-container save/restore on back/forward.
  if ('scrollRestoration' in history) history.scrollRestoration = 'manual'

  // Register global directives
  app.directive('safe-html', vSafeHtml)
  app.directive('twemoji', vTwemoji)
  app.directive('prefetch', vPrefetch)
  app.directive('scroll-restore', vScrollRestore)

  const pinia = createPinia()
  app.use(pinia)

  // Fluent-based i18n. Must register after Pinia (the i18n module
  // reads dateStore.locale for the active bundle) and before
  // components render so the first paint already speaks the right
  // language. Locale follows dateStore.locale, which auth.ts seeds
  // from /me's effective_locale on login.
  app.use(createI18nPlugin(pinia))
  // Pinia Colada must register AFTER Pinia. Provides the canonical
  // async data layer (queries, mutations, optimistic updates,
  // query cache, route loader integration).
  app.use(PiniaColada, {})
  app.use(router)

  // Initialize theme store to respect system preferences for guests
  // This ensures dark mode works even when not logged in
  useThemeStore(pinia)

  // Fetch instance config (routing topology) in parallel with the initial route
  // resolution so its value is settled before first paint. The fetch defaults to
  // 'host' and never rejects, so it can't block or break the mount.
  await Promise.all([fetchInstanceConfig(), router.isReady()])
  app.mount('#app')

  // Signal the launch splash (public/splash.js) that the first real
  // screen is up so it can hand off. Double-rAF after nextTick waits
  // for the mounted screen to actually paint, so the fade never starts
  // over a blank frame. Whichever screen mounted first (connect-server,
  // login, or the app shell) is a real branded screen; auth resolving
  // later doesn't matter here.
  await nextTick()
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      document.getElementById('app-splash')?.setAttribute('data-ready', '')
    }),
  )
}

void bootstrap()
