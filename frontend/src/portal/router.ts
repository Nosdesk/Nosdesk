import { createRouter, createWebHistory } from 'vue-router'

import LoginView from './views/LoginView.vue'
import NewTicketView from './views/NewTicketView.vue'
import TicketsView from './views/TicketsView.vue'
import TicketView from './views/TicketView.vue'

// Hosted serves the portal at the root of each workspace's own origin;
// self-hosted shares the agent app's origin and serves it under `/portal`.
const base =
  window.location.pathname === '/portal' || window.location.pathname.startsWith('/portal/')
    ? '/portal/'
    : '/'

const router = createRouter({
  history: createWebHistory(base),
  routes: [
    { path: '/', redirect: '/tickets' },
    { path: '/login', name: 'login', component: LoginView },
    { path: '/tickets', name: 'tickets', component: TicketsView },
    // Static `/tickets/new` before the dynamic `/tickets/:id` so it isn't
    // captured as an id.
    { path: '/tickets/new', name: 'ticket-new', component: NewTicketView },
    { path: '/tickets/:id', name: 'ticket', component: TicketView, props: true },
    // The public help centre and guest request form, shared with the agent
    // app. No portal session needed.
    { path: '/help', name: 'help', component: () => import('@/views/public/HelpView.vue') },
    {
      path: '/submit-ticket',
      name: 'submit-ticket',
      component: () => import('@/views/public/GuestTicketSubmitView.vue'),
    },
    {
      path: '/ticket-status/:token',
      name: 'ticket-status',
      component: () => import('@/views/public/GuestTicketStatusView.vue'),
      props: true,
    },
    { path: '/docs', name: 'docs', component: () => import('@/views/public/PublicDocsView.vue') },
    {
      path: '/docs/:slug',
      name: 'doc',
      component: () => import('@/views/public/PublicDocView.vue'),
      props: true,
    },
  ],
})

export default router
