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

import { IN_PORTAL } from '@/components/public/inPortal'

import App from './App.vue'
import router from './router'
import { embedHost, isEmbed } from './embed'
import { browserLocale } from './locale'

window.addEventListener('vite:preloadError', (event) => {
  if (reloadForNewBuild()) event.preventDefault()
})

async function bootstrap(): Promise<void> {
  // The shared stores and services (branding, theme) reach the backend through
  // the core seams, so configure them as the agent app does.
  await configurePlatform()

  const app = createApp(App)
  app.provide(IN_PORTAL, true)
  app.directive('safe-html', vSafeHtml)

  const pinia = createPinia()
  app.use(pinia)

  // Before sign-in there is no profile to read, so speak the browser's
  // language; /me replaces it with the requester's own once signed in.
  useDateStore(pinia).setUserLocale(browserLocale())
  app.use(createI18nPlugin(pinia))
  app.use(PiniaColada, {})
  app.use(router)

  const theme = useThemeStore(pinia)
  void useBrandingStore(pinia).loadBranding()

  if (isEmbed) {
    // Plain links open in a new tab rather than navigating the frame.
    const base = document.createElement('base')
    base.target = '_blank'
    document.head.appendChild(base)
  }
  if (embedHost === 'teams') {
    // Teams keeps its own loading indicator up until we say we've drawn.
    const teams = await import('./teams')
    const context = await teams.initTeams()
    if (context) {
      theme.setTheme(teams.portalTheme(context.theme))
      teams.onTeamsThemeChange((next) => theme.setTheme(teams.portalTheme(next)))
      if (context.locale) useDateStore(pinia).setUserLocale(context.locale)
    } else {
      teams.teamsProblem.value = 'not-in-teams'
    }
    const signedIn = context !== null && (await teams.signInTeams()) === null
    await router.replace(signedIn ? teams.routeForSubPage(context?.subPageId ?? null) : '/teams-status')
    await router.isReady()
    app.mount('#app')
    teams.notifyTeamsReady()
    return
  }
  if (embedHost === 'widget') await router.replace('/embed')
  await router.isReady()
  app.mount('#app')
}

void bootstrap()
