<script setup lang="ts">
/**
 * Discard a draft import from a step's action bar. The confirmation says
 * what goes: the uploaded file and the choices made so far. Nothing was
 * imported, so nothing else changes. The wizard does the delete and
 * returns to Data import.
 */
import { ref } from 'vue'

import Button from '@/components/common/Button.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'

defineOptions({ inheritAttrs: false })

defineProps<{
  loading?: boolean
  disabled?: boolean
}>()

const emit = defineEmits<{
  (e: 'discard'): void
}>()

const show = ref(false)

function confirm() {
  show.value = false
  emit('discard')
}
</script>

<template>
  <!-- $attrs after data-test, so a second copy can name its own. -->
  <Button
    variant="ghost-danger"
    :loading="loading"
    :disabled="disabled"
    data-test="import-discard"
    v-bind="$attrs"
    @click="show = true"
  >
    {{ $t('import-discard') }}
  </Button>
  <ConfirmModal
    :show="show"
    variant="danger"
    :title="$t('import-discard-title')"
    :message="$t('import-discard-message')"
    :confirm-label="$t('import-discard-confirm')"
    @close="show = false"
    @confirm="confirm"
  />
</template>
