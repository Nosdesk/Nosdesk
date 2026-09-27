<template>
  <div class="bg-surface border border-default rounded-xl shadow-sm p-6 sm:p-8 flex flex-col items-center gap-5 text-center">
    <div class="w-12 h-12 rounded-full bg-surface-alt flex items-center justify-center">
      <Icon name="lock" size="lg" class="text-tertiary" />
    </div>
    <div class="flex flex-col gap-1">
      <h2 class="text-lg font-semibold text-primary">{{ title }}</h2>
      <p class="text-sm text-secondary">{{ message }}</p>
    </div>
    <!-- Outside the portal, href rather than to: on a hosted tenant origin
         sign-in is the portal, served on a full page load (see RequesterLink). -->
    <LinkButton v-if="inPortal" to="/login">
      {{ $t('feature-disabled-sign-in') }}
    </LinkButton>
    <LinkButton v-else href="/login">
      {{ $t('feature-disabled-sign-in') }}
    </LinkButton>
  </div>
</template>

<script setup lang="ts">
import LinkButton from '@/components/common/LinkButton.vue'
import Icon from '@/components/common/Icon.vue';
import { useInPortal } from '@/components/public/inPortal';

defineProps<{ title: string; message: string }>();
const inPortal = useInPortal();
</script>
