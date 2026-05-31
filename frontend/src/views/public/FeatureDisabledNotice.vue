<template>
  <div class="bg-surface border border-default rounded-xl shadow-sm p-6 sm:p-8 flex flex-col items-center gap-5 text-center">
    <div class="w-12 h-12 rounded-full bg-surface-alt flex items-center justify-center">
      <svg class="w-6 h-6 text-tertiary" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
          d="M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728L5.636 5.636m12.728 12.728A9 9 0 015.636 5.636" />
      </svg>
    </div>
    <div class="flex flex-col gap-1">
      <h2 class="text-lg font-semibold text-primary">{{ title }}</h2>
      <p class="text-sm text-secondary">{{ message }}</p>
    </div>
    <RouterLink
      to="/login"
      class="inline-flex items-center justify-center px-4 py-2 bg-accent hover:opacity-90 text-on-accent rounded-lg text-sm font-medium transition-colors"
    >
      {{ $t('feature-disabled-sign-in') }}
    </RouterLink>
  </div>
</template>

<script setup lang="ts">
import { RouterLink } from 'vue-router';

defineProps<{ title: string; message: string }>();
</script>
