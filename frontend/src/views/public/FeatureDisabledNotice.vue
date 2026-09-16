<template>
  <div class="bg-surface border border-default rounded-xl shadow-sm p-6 sm:p-8 flex flex-col items-center gap-5 text-center">
    <div class="w-12 h-12 rounded-full bg-surface-alt flex items-center justify-center">
      <Icon name="lock" size="lg" class="text-tertiary" />
    </div>
    <div class="flex flex-col gap-1">
      <h2 class="text-lg font-semibold text-primary">{{ title }}</h2>
      <p class="text-sm text-secondary">{{ message }}</p>
    </div>
    <LinkButton
      to="/login"
    >
      {{ $t('feature-disabled-sign-in') }}
    </LinkButton>
  </div>
</template>

<script setup lang="ts">
import LinkButton from '@/components/common/LinkButton.vue'
import Icon from '@/components/common/Icon.vue';

defineProps<{ title: string; message: string }>();
</script>
