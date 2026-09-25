import '../assets/main.css'

import { createApp } from 'vue'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'

import { configurePlatform } from '@/platform'
import { vSafeHtml } from '@/directives/vSafeHtml'
import { createI18n as createI18nPlugin } from '@/i18n'
import { useThemeStore } from '@/stores/theme'
import { useBrandingStore } from '@/stores/branding'
import { reloadForNewBuild } from '@/utils/staleBuild'
import { useDateStore } from '@nosdesk/core/stores/dateStore'

import App from './App.vue'
import router from './router'
import { browserLocale } from './locale'

window.addEventListener('vite:preloadError', (event) => {
  if (reloadForNewBuild()) event.preventDefault()
})

async function bootstrap(): Promise<void> {
  // The shared stores and services (branding, theme) reach the backend through
  // the core seams, so configure them as the agent app does.
  await configurePlatform()

  const app = createApp(App)
  app.directive('safe-html', vSafeHtml)

  const pinia = createPinia()
  app.use(pinia)

  // Before sign-in there is no profile to read, so speak the browser's
  // language; /me replaces it with the requester's own once signed in.
  useDateStore(pinia).setUserLocale(browserLocale())
  app.use(createI18nPlugin(pinia))
  app.use(PiniaColada, {})
  app.use(router)

  useThemeStore(pinia)
  void useBrandingStore(pinia).loadBranding()

  await router.isReady()
  app.mount('#app')
}

void bootstrap()
