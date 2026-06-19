<script setup lang="ts">
import { ref } from 'vue'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'

import { requestMagicLink } from '../service'

const email = ref('')
const sent = ref(false)
const submitting = ref(false)

async function submit(): Promise<void> {
  if (!email.value.trim()) return
  submitting.value = true
  try {
    await requestMagicLink(email.value.trim())
  } finally {
    // Uniform outcome: we always show the "check your email" state, never
    // revealing whether the address is a known customer.
    sent.value = true
    submitting.value = false
  }
}
</script>

<template>
  <div class="min-h-screen flex items-center justify-center p-4">
    <div class="w-full max-w-sm">
      <h1 class="text-xl font-semibold mb-2">Sign in to support</h1>
      <p class="text-sm text-secondary mb-6">
        Enter your email and we'll send you a sign-in link.
      </p>

      <p v-if="sent" class="text-sm">
        If an account exists for that email, a sign-in link is on its way. Check
        your inbox.
      </p>
      <form v-else class="flex flex-col gap-4" @submit.prevent="submit">
        <FormInput
          v-model="email"
          type="email"
          label="Email"
          placeholder="you@example.com"
          required
          :disabled="submitting"
        />
        <Button type="submit" :loading="submitting">Send sign-in link</Button>
      </form>
    </div>
  </div>
</template>
