import { createRouter, createWebHistory } from 'vue-router'

import LoginView from './views/LoginView.vue'
import TicketsView from './views/TicketsView.vue'
import TicketView from './views/TicketView.vue'

// The portal is served at the root of its own per-tenant origin
// (`<slug>.nosdesk.app`), so history base is '/'.
const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', redirect: '/tickets' },
    { path: '/login', name: 'login', component: LoginView },
    { path: '/tickets', name: 'tickets', component: TicketsView },
    { path: '/tickets/:id', name: 'ticket', component: TicketView, props: true },
  ],
})

export default router
