import './assets/main.css'
import './services/apiConfig' // Import axios configuration

import { createApp } from 'vue'
import { createPinia } from 'pinia'

import App from './App.vue'
import router from './router'

const app = createApp(App)

// Enable Vue DevTools in development
if (import.meta.env.DEV) {
  app.config.devtools = true
}

app.use(createPinia())
app.use(router)

// Wait for initial route resolution before mounting
router.isReady().then(() => {
  app.mount('#app')
})
