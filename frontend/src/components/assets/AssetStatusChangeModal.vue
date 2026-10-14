<script setup lang="ts">
/**
 * Move an asset to another status, with a reason, an optional ticket and
 * the details the target needs (repair vendor, disposal record). On-loan is
 * owned by the loan ledger, so it is offered only to restore an asset whose
 * loan is still open. Opened from the asset page's state header.
 */
import { computed, ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import Autocomplete from '@/components/common/Autocomplete.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Button from '@/components/common/Button.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import DatePicker from '@/components/common/DatePicker.vue';
import Modal from '@/components/Modal.vue';
import {
  assetLifecycleKeys,
  assetLifecycleService,
} from '@nosdesk/core/services/assetLifecycleService';
import { metaForAssetStatus } from '@/utils/assetStatusMeta';
import { extractErrorMessage } from '@/utils/errors';
import { ASSET_STATUSES, type AssetStatus } from '@nosdesk/core/types/asset';

const props = defineProps<{
  show: boolean;
  assetId: number;
  currentStatus: string;
  hasActiveLoan: boolean;
}>();
const emit = defineEmits<{ (e: 'close'): void; (e: 'transitioned', toStatus: string): void }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const queryCache = useQueryCache();

// Prior ITAD vendors for the disposal-form suggestions (cached
// workspace-wide; a new vendor can still be typed).
const itadVendorsQuery = useQuery({
  key: () => assetLifecycleKeys.itadVendors,
  query: () => assetLifecycleService.listItadVendors(),
  enabled: () => props.show,
});
const itadVendors = computed<string[]>(() =>
  Array.isArray(itadVendorsQuery.data.value) ? itadVendorsQuery.data.value : [],
);

const submitting = ref(false);
const errorMessage = ref('');

const toStatus = ref<AssetStatus | ''>('');
const reason = ref('');
const ticketIdInput = ref('');
const repairVendor = ref('');
const repairRma = ref('');
const repairOffsite = ref(false);
const repairExpectedReturn = ref('');
const sanitizationMethod = ref('clear');
const dataBearing = ref(true);
const itadVendor = ref('');
const disposalNotes = ref('');

const statusOptions = computed(() =>
  ASSET_STATUSES.filter((s) => {
    if (s === props.currentStatus) return false;
    if (s === 'on_loan') return props.hasActiveLoan;
    return true;
  }),
);
const statusDropdownOptions = computed(() =>
  statusOptions.value.map((status) => ({
    value: status,
    label: t(metaForAssetStatus(status).labelKey),
  })),
);
const sanitizationOptions = computed(() =>
  ['clear', 'purge', 'destroy', 'none'].map((m) => ({
    value: m,
    label: t(`asset-disposal-method-${m}`),
  })),
);

function resetForm() {
  toStatus.value = statusOptions.value[0] ?? '';
  reason.value = '';
  ticketIdInput.value = '';
  repairVendor.value = '';
  repairRma.value = '';
  repairOffsite.value = false;
  repairExpectedReturn.value = '';
  sanitizationMethod.value = 'clear';
  dataBearing.value = true;
  itadVendor.value = '';
  disposalNotes.value = '';
  errorMessage.value = '';
}

// A fresh form each time the dialog opens.
watch(
  () => props.show,
  (open) => {
    if (open) resetForm();
  },
  { immediate: true },
);

function buildMetadata(): Record<string, unknown> {
  if (toStatus.value === 'in_repair') {
    const meta: Record<string, unknown> = {};
    if (repairVendor.value.trim()) meta.vendor = repairVendor.value.trim();
    if (repairRma.value.trim()) meta.rma_number = repairRma.value.trim();
    if (repairOffsite.value) meta.offsite = true;
    if (repairExpectedReturn.value) meta.expected_return = repairExpectedReturn.value;
    return meta;
  }
  return {};
}

function buildDisposal() {
  if (toStatus.value !== 'disposed') return undefined;
  return {
    sanitization_method: sanitizationMethod.value,
    data_bearing: dataBearing.value,
    itad_vendor: itadVendor.value.trim() || null,
    notes: disposalNotes.value.trim() || null,
  };
}

function parseTicketId(): number | null {
  const raw = ticketIdInput.value.trim();
  if (!raw) return null;
  const n = Number.parseInt(raw, 10);
  return Number.isFinite(n) && n > 0 ? n : null;
}

async function submitTransition() {
  if (!toStatus.value) return;
  submitting.value = true;
  errorMessage.value = '';
  try {
    const newStatus = toStatus.value;
    await assetLifecycleService.transition(props.assetId, {
      to_status: newStatus,
      reason: reason.value.trim() || null,
      ticket_id: parseTicketId(),
      metadata: buildMetadata(),
      disposal: buildDisposal(),
    });
    await queryCache.invalidateQueries({ key: assetLifecycleKeys.forAsset(props.assetId) });
    emit('transitioned', newStatus);
  } catch (e) {
    // The server explains refusals (disposal is final, the asset is out on
    // loan, a missing disposal record), so show its message.
    errorMessage.value = extractErrorMessage(e, t('asset-lifecycle-transition-failed'));
  } finally {
    submitting.value = false;
  }
}

</script>

<template>
  <Modal
    :show="show"
    :title="$t('asset-lifecycle-modal-title')"
    size="md"
    @close="emit('close')"
  >
    <div class="flex flex-col gap-4">
      <BaseDropdown
        :model-value="toStatus"
        :options="statusDropdownOptions"
        :label="$t('asset-lifecycle-modal-to-status')"
        size="sm"
        @update:model-value="toStatus = String($event) as AssetStatus"
      />

      <FormTextarea
        v-model="reason"
        :label="$t('asset-lifecycle-modal-reason')"
        :placeholder="$t('asset-lifecycle-modal-reason-placeholder')"
        :rows="3"
      />

      <FormInput
        v-model="ticketIdInput"
        :label="$t('asset-lifecycle-modal-ticket')"
        :placeholder="$t('asset-lifecycle-modal-ticket-placeholder')"
        inputmode="numeric"
      />

      <template v-if="toStatus === 'in_repair'">
        <FormInput
          v-model="repairVendor"
          :label="$t('asset-lifecycle-meta-vendor')"
        />
        <FormInput
          v-model="repairRma"
          :label="$t('asset-lifecycle-meta-rma')"
        />
        <Checkbox
          v-model="repairOffsite"
          :label="$t('asset-lifecycle-meta-offsite')"
        />
        <DatePicker
          v-model="repairExpectedReturn"
          :label="$t('asset-lifecycle-meta-expected-return')"
        />
      </template>

      <template v-if="toStatus === 'disposed'">
        <BaseDropdown
          :model-value="sanitizationMethod"
          :options="sanitizationOptions"
          :label="$t('asset-disposal-method')"
          size="sm"
          @update:model-value="sanitizationMethod = String($event)"
        />
        <Checkbox
          v-model="dataBearing"
          :label="$t('asset-disposal-data-bearing')"
        />
        <Autocomplete
          v-model="itadVendor"
          :options="itadVendors"
          :label="$t('asset-disposal-itad-vendor')"
        />
        <FormTextarea
          v-model="disposalNotes"
          :label="$t('asset-disposal-notes')"
          :rows="2"
        />
      </template>

      <p v-if="errorMessage" class="text-sm text-status-error">{{ errorMessage }}</p>
    </div>

    <template #footer>
      <div class="modal-actions">
        <Button variant="secondary" @click="emit('close')">
          {{ $t('asset-lifecycle-modal-cancel') }}
        </Button>
        <Button :loading="submitting" :disabled="!toStatus" @click="submitTransition">
          {{ $t('asset-lifecycle-modal-submit') }}
        </Button>
      </div>
    </template>
  </Modal>
</template>
