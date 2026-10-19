<!--
  Cloud only: how to reach the Nosdesk team about the account itself
  (plan, billing, cancelling), which the in-app bug report is not for.
-->
<template>
  <Modal :show="isOpen" :title="$t('contact-support-title')" size="sm" @close="emit('close')">
    <div class="flex flex-col gap-3">
      <p class="text-sm text-secondary">{{ $t('contact-support-body') }}</p>
      <SupportAddress />
    </div>
  </Modal>
</template>

<script setup lang="ts">
import Modal from '@/components/Modal.vue'
import SupportAddress from '@/components/SupportAddress.vue'

defineProps<{ isOpen: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()
</script>
