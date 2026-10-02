import { createMemoryHistory, createRouter, createWebHistory } from 'vue-router'

import { isEmbed } from './embed'

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
  // Embedded (the widget, the Teams tab) the frame keeps one URL: navigation
  // stays in memory, so it never reloads onto a page that refuses framing.
  history: isEmbed ? createMemoryHistory() : createWebHistory(base),
  routes: [
    { path: '/embed', name: 'embed', component: () => import('./views/EmbedHomeView.vue') },
    // The Teams tab when it can't sign the person in on its own.
    {
      path: '/teams-status',
      name: 'teams-status',
      component: () => import('./views/TeamsStatusView.vue'),
    },
    { path: '/', redirect: '/tickets' },
    { path: '/login', name: 'login', component: LoginView },
    { path: '/tickets', name: 'tickets', component: TicketsView },
    { path: '/tickets/new', name: 'ticket-new', component: NewTicketView },
    // The request's number, as the requester quotes it.
    { path: '/tickets/:number(\\d+)', name: 'ticket', component: TicketView, props: true },
    // Requests waiting for the signed-in person's approval.
    { path: '/approvals', name: 'approvals', component: () => import('./views/ApprovalsView.vue') },
    {
      path: '/approvals/:id',
      name: 'approval',
      component: () => import('./views/ApprovalView.vue'),
      props: true,
    },
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
