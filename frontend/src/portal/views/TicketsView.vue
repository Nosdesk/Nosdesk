<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { RouterLink } from 'vue-router'

import { listMyTickets, type PortalTicket } from '../service'

const tickets = ref<PortalTicket[]>([])
const loading = ref(true)
const failed = ref(false)

onMounted(async () => {
  try {
    tickets.value = await listMyTickets()
  } catch {
    failed.value = true
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div class="max-w-2xl mx-auto p-4">
    <div class="flex items-center mb-4">
      <h1 class="text-xl font-semibold">My tickets</h1>
      <RouterLink to="/tickets/new" class="ml-auto text-sm text-accent hover:underline">
        New ticket
      </RouterLink>
    </div>

    <p v-if="loading" class="text-sm text-secondary">Loading your tickets…</p>
    <p v-else-if="failed" class="text-sm text-status-error">
      We couldn't load your tickets. Please try again.
    </p>
    <ul v-else-if="tickets.length" class="flex flex-col gap-2">
      <li v-for="t in tickets" :key="t.id">
        <RouterLink
          :to="`/tickets/${t.id}`"
          class="block border border-border rounded-md p-3 hover:bg-surface-hover"
        >
          <span class="font-medium">{{ t.title }}</span>
        </RouterLink>
      </li>
    </ul>
    <p v-else class="text-sm text-secondary">You have no tickets yet.</p>
  </div>
</template>
