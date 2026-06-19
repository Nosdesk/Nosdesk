<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'

import { createMyTicket } from '../service'

const router = useRouter()

const title = ref('')
const description = ref('')
const submitting = ref(false)
const failed = ref(false)

async function submit(): Promise<void> {
  if (!title.value.trim()) return
  submitting.value = true
  failed.value = false
  try {
    const ticket = await createMyTicket(title.value.trim(), description.value.trim())
    router.push(`/tickets/${ticket.id}`)
  } catch {
    failed.value = true
    submitting.value = false
  }
}
</script>

<template>
  <div class="max-w-2xl mx-auto p-4">
    <RouterLink to="/tickets" class="text-sm text-accent hover:underline">
      &larr; Back to my tickets
    </RouterLink>

    <h1 class="text-xl font-semibold mt-4 mb-4">New ticket</h1>

    <form class="flex flex-col gap-4" @submit.prevent="submit">
      <FormInput
        v-model="title"
        label="Subject"
        placeholder="A short summary"
        required
        :disabled="submitting"
      />
      <FormTextarea
        v-model="description"
        label="How can we help?"
        placeholder="Describe your issue"
        :rows="5"
        :disabled="submitting"
      />
      <p v-if="failed" class="text-sm text-status-error">
        Something went wrong. Please try again.
      </p>
      <div class="flex">
        <Button type="submit" :loading="submitting" class="ml-auto">
          Submit ticket
        </Button>
      </div>
    </form>
  </div>
</template>
