<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { RouterLink } from 'vue-router'

import { getMyTicket, type PortalComment, type PortalTicket } from '../service'

const props = defineProps<{ id: string }>()

const ticket = ref<PortalTicket | null>(null)
const comments = ref<PortalComment[]>([])
const loading = ref(true)
const failed = ref(false)

onMounted(async () => {
  try {
    const detail = await getMyTicket(Number(props.id))
    ticket.value = detail.ticket
    comments.value = detail.comments
  } catch {
    failed.value = true
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div class="max-w-2xl mx-auto p-4">
    <RouterLink to="/tickets" class="text-sm text-accent hover:underline">
      &larr; Back to my tickets
    </RouterLink>

    <p v-if="loading" class="text-sm text-secondary mt-4">Loading…</p>
    <p v-else-if="failed || !ticket" class="text-sm text-status-error mt-4">
      This ticket couldn't be loaded.
    </p>
    <div v-else class="mt-4">
      <h1 class="text-xl font-semibold mb-4">{{ ticket.title }}</h1>
      <ul class="flex flex-col gap-3">
        <li
          v-for="c in comments"
          :key="c.id"
          class="border border-border rounded-md p-3"
        >
          <p class="whitespace-pre-wrap text-sm">{{ c.content }}</p>
          <p class="text-xs text-secondary mt-2">{{ c.created_at }}</p>
        </li>
      </ul>
      <p v-if="!comments.length" class="text-sm text-secondary">
        No messages on this ticket yet.
      </p>
    </div>
  </div>
</template>
