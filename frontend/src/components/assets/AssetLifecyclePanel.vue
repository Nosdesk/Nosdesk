<script setup lang="ts">
import { computed, ref } from 'vue';
import { RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Button from '@/components/common/Button.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import DatePicker from '@/components/common/DatePicker.vue';
import Icon from '@/components/common/Icon.vue';
import Modal from '@/components/Modal.vue';
import AssetStatusBadge from '@/components/assets/AssetStatusBadge.vue';
import {
  assetLifecycleKeys,
  assetLifecycleService,
} from '@/services/assetLifecycleService';
import { useSyncActions } from '@/composables/useSyncActions';
import { useUsersDirectory } from '@/composables/useUsersDirectory';
import { metaForAssetStatus } from '@/utils/assetStatusMeta';
import { formatRelativeTime } from '@/utils/dateUtils';
import { ASSET_STATUSES, type AssetLifecycleEvent, type AssetStatus } from '@/types/asset';

const props = defineProps<{
  assetId: number;
  currentStatus: string;
  canEdit?: boolean;
}>();

// Emitted after a successful status transition so the parent can refresh
// the asset (its `currentStatus` prop, and the status badge it drives).
const emit = defineEmits<{ (e: 'transitioned', toStatus: string): void }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const { getUserHandle } = useUsersDirectory();

const queryCache = useQueryCache();
const lifecycleQuery = useQuery({
  key: () => assetLifecycleKeys.forAsset(props.assetId),
  query: () => assetLifecycleService.list(props.assetId),
});
const events = computed<AssetLifecycleEvent[]>(() =>
  Array.isArray(lifecycleQuery.data.value) ? lifecycleQuery.data.value : [],
);
const isFirstLoad = computed(
  () => lifecycleQuery.status.value === 'pending' && lifecycleQuery.data.value === undefined,
);

function invalidate() {
  return queryCache.invalidateQueries({ key: assetLifecycleKeys.forAsset(props.assetId) });
}

useSyncActions(
  (actions) => {
    if (
      actions.some((a) => {
        const data = a.data as { asset_id?: number };
        return data.asset_id === props.assetId;
      })
    ) {
      void invalidate();
    }
  },
  { aggregates: ['asset_lifecycle_event'], debounceMs: 250 },
);

const showModal = ref(false);
const submitting = ref(false);
const errorMessage = ref('');

const toStatus = ref<AssetStatus | ''>('');
const reason = ref('');
const ticketIdInput = ref('');
const repairVendor = ref('');
const repairRma = ref('');
const repairOffsite = ref(false);
const repairExpectedReturn = ref('');

// On-loan is owned by the loan ledger (the Loans panel), not a manual
// transition: you loan an asset out and return it there, which keeps the
// loan record and the status in step. So it's not offered as a target, and
// while an asset is on loan its status is changed by returning the loan.
const isOnLoan = computed(() => props.currentStatus === 'on_loan');
const statusOptions = computed(() =>
  ASSET_STATUSES.filter((s) => s !== props.currentStatus && s !== 'on_loan'),
);
const statusDropdownOptions = computed(() =>
  statusOptions.value.map((status) => ({
    value: status,
    label: t(metaForAssetStatus(status).labelKey),
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
  errorMessage.value = '';
}

function openModal() {
  resetForm();
  showModal.value = true;
}

function closeModal() {
  showModal.value = false;
  errorMessage.value = '';
}

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
    });
    await invalidate();
    emit('transitioned', newStatus);
    closeModal();
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('asset-lifecycle-transition-failed');
  } finally {
    submitting.value = false;
  }
}

function actorLabel(uuid: string | null | undefined): string {
  if (!uuid) return t('asset-lifecycle-timeline-unknown-actor');
  const handle = getUserHandle(uuid);
  return handle.user.value?.name ?? t('asset-lifecycle-timeline-unknown-actor');
}

function transitionSummary(event: AssetLifecycleEvent): string {
  const toMeta = metaForAssetStatus(event.to_status);
  const toLabel = t(toMeta.labelKey);
  if (event.from_status) {
    const fromMeta = metaForAssetStatus(event.from_status);
    return t('asset-lifecycle-timeline-transition', {
      from: t(fromMeta.labelKey),
      to: toLabel,
    });
  }
  return t('asset-lifecycle-timeline-initial', { to: toLabel });
}

function metadataLines(event: AssetLifecycleEvent): string[] {
  const meta = event.metadata ?? {};
  const lines: string[] = [];
  if (typeof meta.vendor === 'string' && meta.vendor) {
    lines.push(t('asset-lifecycle-timeline-vendor', { vendor: meta.vendor }));
  }
  if (typeof meta.rma_number === 'string' && meta.rma_number) {
    lines.push(t('asset-lifecycle-timeline-rma', { rma: meta.rma_number }));
  }
  if (meta.offsite === true) {
    lines.push(t('asset-lifecycle-timeline-offsite'));
  }
  if (typeof meta.expected_return === 'string' && meta.expected_return) {
    lines.push(t('asset-lifecycle-timeline-expected-return', { date: meta.expected_return }));
  }
  if (typeof meta.loaned_to === 'string' && meta.loaned_to) {
    lines.push(t('asset-lifecycle-timeline-loaned-to', { name: meta.loaned_to }));
  }
  if (typeof meta.due_back === 'string' && meta.due_back) {
    lines.push(t('asset-lifecycle-timeline-due-back', { date: meta.due_back }));
  }
  return lines;
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <div class="flex items-center justify-between gap-3 flex-wrap">
      <AssetStatusBadge :status="currentStatus" size="md" />
      <Button
        v-if="canEdit && !isOnLoan"
        size="sm"
        icon="refresh"
        @click="openModal"
      >
        {{ $t('asset-lifecycle-change-status') }}
      </Button>
      <span v-else-if="canEdit && isOnLoan" class="text-xs text-tertiary">
        {{ $t('asset-lifecycle-managed-by-loan') }}
      </span>
    </div>

    <p class="text-xs text-tertiary">{{ $t('asset-lifecycle-description') }}</p>

    <div
      v-if="events.length === 0 && !isFirstLoad"
      class="rounded-lg border border-dashed border-default bg-surface-alt p-4 flex items-start gap-3"
    >
      <Icon name="history" class="text-tertiary flex-shrink-0 mt-0.5" />
      <div class="min-w-0">
        <p class="text-sm font-medium text-primary">{{ $t('asset-lifecycle-empty-title') }}</p>
        <p class="text-xs text-tertiary mt-1">{{ $t('asset-lifecycle-empty-description') }}</p>
      </div>
    </div>

    <div v-else-if="events.length > 0" class="divide-y divide-default">
      <div
        v-for="event in events"
        :key="event.id"
        class="py-2.5 flex flex-col gap-1"
      >
        <div class="flex items-baseline justify-between gap-3">
          <span class="text-sm font-medium text-primary">{{ transitionSummary(event) }}</span>
          <span class="text-xs text-tertiary whitespace-nowrap">
            {{ formatRelativeTime(event.occurred_at, { addSuffix: true }) }}
          </span>
        </div>
        <div class="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs">
          <span class="text-tertiary">{{ $t('asset-lifecycle-timeline-actor', { name: actorLabel(event.actor_uuid) }) }}</span>
          <RouterLink
            v-if="event.ticket_id"
            :to="`/tickets/${event.ticket_id}`"
            class="inline-flex items-center px-1.5 py-0.5 rounded border border-default bg-surface-alt text-accent hover:underline"
          >
            {{ $t('asset-lifecycle-timeline-ticket', { id: event.ticket_id }) }}
          </RouterLink>
          <span v-if="event.reason" class="text-secondary">{{ event.reason }}</span>
        </div>
        <div v-if="metadataLines(event).length" class="flex flex-col gap-0.5 text-xs text-secondary">
          <span v-for="(line, idx) in metadataLines(event)" :key="idx">{{ line }}</span>
        </div>
      </div>
    </div>

    <Modal
      :show="showModal"
      :title="$t('asset-lifecycle-modal-title')"
      size="md"
      @close="closeModal"
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

        <p v-if="errorMessage" class="text-sm text-status-error">{{ errorMessage }}</p>
      </div>

      <template #footer>
        <div class="modal-actions">
          <Button variant="secondary" @click="closeModal">
            {{ $t('asset-lifecycle-modal-cancel') }}
          </Button>
          <Button :loading="submitting" :disabled="!toStatus" @click="submitTransition">
            {{ $t('asset-lifecycle-modal-submit') }}
          </Button>
        </div>
      </template>
    </Modal>
  </div>
</template>
