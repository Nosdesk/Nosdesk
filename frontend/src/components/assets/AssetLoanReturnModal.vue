<script setup lang="ts">
/** Return an asset's open loan, with optional notes. */
import { ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import Button from '@/components/common/Button.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import Modal from '@/components/Modal.vue';
import { assetLoanService } from '@nosdesk/core/services/assetLoanService';
import type { AssetLoan } from '@nosdesk/core/types/asset';

const props = defineProps<{
  show: boolean;
  assetId: number;
  loan: AssetLoan | null;
  borrowerName: string;
}>();
const emit = defineEmits<{ (e: 'close'): void; (e: 'returned'): void }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const returnNotes = ref('');
const returnError = ref('');
const submitting = ref(false);

watch(
  () => props.show,
  (open) => {
    if (!open) return;
    returnNotes.value = '';
    returnError.value = '';
  },
  { immediate: true },
);

async function submitReturn() {
  const loan = props.loan;
  if (!loan) return;
  submitting.value = true;
  returnError.value = '';
  try {
    await assetLoanService.returnLoan(props.assetId, loan.id, {
      notes: returnNotes.value.trim() || null,
    });
    emit('returned');
  } catch (e) {
    returnError.value = e instanceof Error ? e.message : t('asset-loan-failed');
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <Modal :show="show" :title="$t('asset-loan-return-title')" size="md" @close="emit('close')">
    <div class="flex flex-col gap-4">
      <p v-if="loan" class="text-sm text-secondary">
        {{ $t('asset-loan-return-body', { name: borrowerName }) }}
      </p>
      <FormTextarea v-model="returnNotes" :label="$t('asset-loan-return-notes')" :rows="2" />
      <p v-if="returnError" class="text-sm text-status-error">{{ returnError }}</p>
    </div>
    <template #footer>
      <div class="modal-actions">
        <Button variant="secondary" @click="emit('close')">{{ $t('asset-loan-cancel') }}</Button>
        <Button :loading="submitting" @click="submitReturn">{{ $t('asset-loan-return') }}</Button>
      </div>
    </template>
  </Modal>
</template>
