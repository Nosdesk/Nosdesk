import './assets/main.css'
import './services/apiConfig' // Import axios configuration

import { createApp } from 'vue'
import { createPinia } from 'pinia'

import App from './App.vue'
import router from './router'

const app = createApp(App)

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
