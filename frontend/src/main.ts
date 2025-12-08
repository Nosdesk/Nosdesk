import './assets/main.css'
import './services/apiConfig' // Import axios configuration

// Initialise remote logging for debugging (can be disabled via localStorage)
import { interceptConsole } from './utils/remoteLogger'
interceptConsole()

import { createApp } from 'vue'
import { createPinia } from 'pinia'

import App from './App.vue'
import router from './router'
import { vSafeHtml } from './directives/vSafeHtml'

const app = createApp(App)

// Register global directives
app.directive('safe-html', vSafeHtml)

const pinia = createPinia()
app.use(pinia)
app.use(router)

// Initialize theme store to respect system preferences for guests
// This ensures dark mode works even when not logged in
import { useThemeStore } from './stores/theme'
useThemeStore(pinia)

// Wait for initial route resolution before mounting
router.isReady().then(() => {
  app.mount('#app')
})
