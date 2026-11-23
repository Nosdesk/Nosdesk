<script setup lang="ts">
/**
 * Move an asset to another status, with a reason, an optional ticket and
 * the details the target needs (a repair record, a disposal record). On-loan
 * is owned by the loan ledger, so it is offered only to restore an asset
 * whose loan is still open. Opened from the asset page's state header.
 *
 * Moving into a status a record type opens with (repair, unless an admin
 * pointed another type at it) offers to open a record of that type, ticked;
 * the workspace adds the built-in repair type the first time. Moving out
 * offers to complete the open record (of the type that status opens, or
 * opened by a move into it), with a picker when there are several. Save
 * waits until the types and open records have loaded, so a quick submit
 * can't skip either section.
 */
import { computed, ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { formatDate } from '@nosdesk/core/utils/dateUtils';
import { readAmountInput, type AmountInputError } from '@nosdesk/core/utils/money';
import Autocomplete from '@/components/common/Autocomplete.vue';
import BaseDropdown from '@/components/common/BaseDropdown.vue';
import Button from '@/components/common/Button.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import DatePicker from '@/components/common/DatePicker.vue';
import DynamicAttributeForm, { type Schema } from '@/components/assets/DynamicAttributeForm.vue';
import Modal from '@/components/Modal.vue';
import {
  assetLifecycleKeys,
  assetLifecycleService,
  type LifecycleTransitionBody,
} from '@nosdesk/core/services/assetLifecycleService';
import { assetRecordKeys, assetRecordService } from '@nosdesk/core/services/assetRecordService';
import { useWorkspaceQuery, useWorkspaceQueryCache } from '@/queries/workspaceQuery';
import { metaForAssetStatus } from '@/utils/assetStatusMeta';
import { extractErrorMessage } from '@/utils/errors';
import {
  ASSET_STATUSES,
  type AssetRecord,
  type AssetRecordCoreField,
  type AssetRecordCoreFieldMode,
  type AssetRecordOutcome,
  type AssetStatus,
} from '@nosdesk/core/types/asset';

const props = defineProps<{
  show: boolean;
  assetId: number;
  currentStatus: string;
  hasActiveLoan: boolean;
  /** The asset's kind, so a record type not offered for it isn't either. */
  assetKind?: string;
}>();
const emit = defineEmits<{ (e: 'close'): void; (e: 'transitioned', toStatus: string): void }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const queryCache = useQueryCache();
const workspaceCache = useWorkspaceQueryCache();

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

/** The record types a move opens or completes, archived ones too (an open
 *  record can have one), with the workspace currency for a cost. */
const recordTypesQuery = useWorkspaceQuery({
  key: [...assetRecordKeys.types, 'with-archived'],
  query: () => assetRecordService.types(true),
  enabled: () => props.show,
});
/** The built-ins, for a type the workspace hasn't added yet. */
const catalogueQuery = useWorkspaceQuery({
  key: [...assetRecordKeys.catalogue],
  query: () => assetRecordService.catalogue(),
  enabled: () => props.show,
});

/** What the dialog needs of a record type, whether the workspace has it or
 *  a move would add it from the catalogue. */
interface OpenableType {
  slug: string;
  label: string | null;
  field_schema: Record<string, unknown>;
  field_order: string[];
  core_fields: Partial<Record<AssetRecordCoreField, AssetRecordCoreFieldMode>>;
  outcomes: AssetRecordOutcome[];
  applies_to_kinds: string[] | null;
}

/** The live type a move into `status` opens a record of. Failing that, the
 *  catalogue entry the move would add, when the workspace never added it; one
 *  it added and archived, or pointed elsewhere, stays out. */
function typeOpenedBy(status: string): OpenableType | null {
  const live = (recordTypesQuery.data.value?.types ?? []).find(
    (type) => type.opens_with_status === status && !type.archived_at,
  );
  if (live) return live;
  const entry = (catalogueQuery.data.value ?? []).find(
    (e) => e.opens_with_status === status && !e.added,
  );
  return entry ? { ...entry, label: null, applies_to_kinds: null } : null;
}

function offeredForKind(type: OpenableType | null): OpenableType | null {
  if (!type) return null;
  if (type.applies_to_kinds && props.assetKind && !type.applies_to_kinds.includes(props.assetKind)) {
    return null;
  }
  return type;
}

/** The asset's open records a move out of its status completes: of the
 *  type that status opens, or opened by a move into it. */
const openRecordsQuery = useWorkspaceQuery({
  key: () => [...assetRecordKeys.forAsset(props.assetId), 'open', 'leaving', props.currentStatus],
  query: () =>
    assetRecordService.list(props.assetId, { open: true, leaving: props.currentStatus }),
  enabled: () => props.show,
});
const openRecords = computed<AssetRecord[]>(() => openRecordsQuery.data.value?.records ?? []);

/** Whether what the move may write is still loading. */
const recordsLoading = computed(
  () =>
    recordTypesQuery.status.value === 'pending' ||
    catalogueQuery.status.value === 'pending' ||
    openRecordsQuery.status.value === 'pending',
);

const currency = computed(
  () => recordTypesQuery.data.value?.currency ?? recordTypesQuery.data.value?.suggested_currency ?? '',
);

function typeName(type: OpenableType): string {
  return type.label ?? t(`asset-record-type-${type.slug}`);
}

/** The built-in repair type under its own name gets the plain wording. */
function isPlainRepair(type: OpenableType): boolean {
  return type.slug === 'repair' && !type.label;
}

function shown(type: OpenableType, field: AssetRecordCoreField): boolean {
  return type.core_fields[field] !== 'hidden';
}

function required(type: OpenableType, field: AssetRecordCoreField): boolean {
  return type.core_fields[field] === 'required';
}

/** The type's fields in their order, each titled: a built-in field with no
 *  title of its own reads in the reader's language. */
function orderedSchema(type: OpenableType): Schema {
  const properties = (type.field_schema.properties ?? {}) as Record<string, Record<string, unknown>>;
  const keys = [
    ...type.field_order.filter((key) => key in properties),
    ...Object.keys(properties).filter((key) => !type.field_order.includes(key)),
  ];
  const ordered: NonNullable<Schema['properties']> = {};
  for (const key of keys) {
    const prop = properties[key] ?? {};
    ordered[key] = {
      ...prop,
      title: (prop.title as string | undefined) ?? t(`asset-record-field-${type.slug}-${key}`),
    };
  }
  return {
    type: 'object',
    properties: ordered,
    required: (type.field_schema.required as string[] | undefined) ?? [],
  };
}

function outcomeLabel(type: OpenableType, outcome: AssetRecordOutcome): string {
  return outcome.label ?? t(`asset-record-outcome-${type.slug}-${outcome.key}`);
}

/** Today in the browser's timezone, as the date picker writes days. */
function today(): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

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
const openRecord = ref(true);
const recordSummary = ref('');
const recordVendor = ref('');
const recordReference = ref('');
const recordAttributes = ref<Record<string, unknown>>({});
const completeRecord = ref(true);
const completeRecordId = ref<number | null>(null);
const completeOutcome = ref('');
const completeCost = ref('');
const completedOn = ref(today());
/** The day the picker started on. Left there, the record completes at the
 *  move itself: the browser's today can fall before the day the server
 *  dated the work, in the workspace's timezone. */
const completedOnDefault = ref(completedOn.value);

/** The type the chosen status opens, when the asset's kind is offered it. */
const enteringType = computed(() =>
  toStatus.value ? offeredForKind(typeOpenedBy(toStatus.value)) : null,
);
/** The open record the move out completes, picked or the only one. */
const completing = computed(() => {
  if (openRecords.value.length === 0) return null;
  if (openRecords.value.length === 1) return openRecords.value[0];
  return openRecords.value.find((r) => r.id === completeRecordId.value) ?? null;
});
/** The type of the record the move completes: the picked one's, else the
 *  first open one's until one is picked. */
const leavingType = computed<OpenableType | null>(() => {
  const record = completing.value ?? openRecords.value[0];
  if (!record) return null;
  return (recordTypesQuery.data.value?.types ?? []).find((t) => t.id === record.record_type_id) ?? null;
});
// Another record's type has other outcomes.
watch(completeRecordId, () => {
  completeOutcome.value = '';
});

/** The cost as typed, read in the reader's locale. */
const costInput = computed(() => {
  const text = completeCost.value.trim();
  return text ? readAmountInput(text) : null;
});
const COST_ERRORS: Record<AmountInputError, string> = {
  invalid: 'asset-lifecycle-record-cost-invalid',
  ambiguous: 'asset-lifecycle-record-cost-ambiguous',
  negative: 'asset-lifecycle-record-cost-negative',
  too_many_decimals: 'asset-lifecycle-record-cost-invalid',
};
const costError = computed(() =>
  costInput.value && !costInput.value.ok ? t(COST_ERRORS[costInput.value.error]) : '',
);

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
const outcomeOptions = computed(() => {
  const type = leavingType.value;
  if (!type) return [];
  return [
    { value: '', label: t('asset-lifecycle-record-outcome-none') },
    ...type.outcomes.map((o) => ({ value: o.key, label: outcomeLabel(type, o) })),
  ];
});

function recordChoiceLabel(record: AssetRecord): string {
  // A bare day: read in UTC so it isn't shifted to the day before.
  const date = formatDate(record.occurred_on, 'date', 'UTC');
  const detail = record.reference ?? record.vendor ?? record.summary;
  return detail
    ? t('asset-lifecycle-record-choice', { date, detail })
    : t('asset-lifecycle-record-choice-plain', { date });
}

const recordChoiceOptions = computed(() =>
  openRecords.value.map((record) => ({ value: record.id, label: recordChoiceLabel(record) })),
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
  openRecord.value = true;
  recordSummary.value = '';
  recordVendor.value = '';
  recordReference.value = '';
  recordAttributes.value = {};
  completeRecord.value = true;
  completeRecordId.value = null;
  completeOutcome.value = '';
  completeCost.value = '';
  completedOn.value = today();
  completedOnDefault.value = completedOn.value;
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

/** Whether the move opens a record: offered and ticked. */
const opensRecord = computed(() => !!enteringType.value && openRecord.value);
/** Whether the move completes a record: there is one open and it's ticked. */
const completesRecord = computed(
  () => !!leavingType.value && openRecords.value.length > 0 && completeRecord.value,
);

/** The old repair details, for a move into repair that opens no record
 *  because no type can (the workspace archived its repair type). */
const legacyRepair = computed(() => toStatus.value === 'in_repair' && !enteringType.value);

/** A value worth sending: not empty text and not an empty list. */
function filled(value: unknown): boolean {
  if (value === undefined || value === null) return false;
  if (typeof value === 'string') return value.trim() !== '';
  if (Array.isArray(value)) return value.length > 0;
  return true;
}

function buildRecord(): LifecycleTransitionBody['record'] {
  if (!opensRecord.value) return undefined;
  const attributes = Object.fromEntries(
    Object.entries(recordAttributes.value).filter(([, value]) => filled(value)),
  );
  return {
    summary: recordSummary.value.trim() || null,
    vendor: recordVendor.value.trim() || null,
    reference: recordReference.value.trim() || null,
    attributes,
  };
}

function buildCompletion(): LifecycleTransitionBody['complete_record'] {
  if (!completesRecord.value) return undefined;
  const cost = costInput.value;
  return {
    record_id: completing.value?.id ?? completeRecordId.value,
    outcome: completeOutcome.value || null,
    cost: cost?.ok ? { amount: cost.amount, currency: currency.value || null } : null,
    completed_on:
      completedOn.value && completedOn.value !== completedOnDefault.value
        ? completedOn.value
        : null,
  };
}

function buildMetadata(): Record<string, unknown> {
  if (legacyRepair.value) {
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

/** The completion can't go until a record is picked from several. */
const needsChoice = computed(
  () => completesRecord.value && openRecords.value.length > 1 && !completing.value,
);
/** Save waits for the record queries, a picked record and a readable cost. */
const blocked = computed(
  () =>
    recordsLoading.value || needsChoice.value || (completesRecord.value && !!costError.value),
);

async function submitTransition() {
  if (!toStatus.value || blocked.value) return;
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
      record: buildRecord(),
      complete_record: buildCompletion(),
    });
    await queryCache.invalidateQueries({ key: assetLifecycleKeys.forAsset(props.assetId) });
    await Promise.all([
      workspaceCache.invalidateQueries({ key: [...assetRecordKeys.forAsset(props.assetId)] }),
      // A first repair record adds the type to the workspace.
      workspaceCache.invalidateQueries({ key: [...assetRecordKeys.types] }),
    ]);
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

      <section
        v-if="leavingType && openRecords.length > 0"
        class="flex flex-col gap-3 border-t border-default pt-4"
        data-testid="complete-record"
      >
        <Checkbox
          v-model="completeRecord"
          :label="isPlainRepair(leavingType)
            ? $t('asset-lifecycle-record-complete')
            : $t('asset-lifecycle-record-complete-type', { type: typeName(leavingType) })"
        />
        <template v-if="completeRecord">
          <BaseDropdown
            v-if="openRecords.length > 1"
            :model-value="completeRecordId ?? ''"
            :options="recordChoiceOptions"
            :label="$t('asset-lifecycle-record-which')"
            :placeholder="$t('asset-lifecycle-record-which-placeholder')"
            size="sm"
            @update:model-value="completeRecordId = Number($event)"
          />
          <BaseDropdown
            v-if="shown(leavingType, 'outcome')"
            :model-value="completeOutcome"
            :options="outcomeOptions"
            :label="$t('asset-lifecycle-record-outcome')"
            size="sm"
            @update:model-value="completeOutcome = String($event)"
          />
          <FormInput
            v-if="shown(leavingType, 'cost')"
            v-model="completeCost"
            :label="$t('asset-lifecycle-record-cost', { currency })"
            inputmode="decimal"
            :required="required(leavingType, 'cost')"
            :error="costError"
          />
          <DatePicker
            v-model="completedOn"
            :label="$t('asset-lifecycle-record-completed-on')"
          />
        </template>
      </section>

      <section
        v-if="enteringType"
        class="flex flex-col gap-3 border-t border-default pt-4"
        data-testid="open-record"
      >
        <Checkbox
          v-model="openRecord"
          :label="isPlainRepair(enteringType)
            ? $t('asset-lifecycle-record-open')
            : $t('asset-lifecycle-record-open-type', { type: typeName(enteringType) })"
        />
        <template v-if="openRecord">
          <FormInput
            v-if="required(enteringType, 'summary')"
            v-model="recordSummary"
            :label="$t('asset-lifecycle-record-summary')"
            required
          />
          <FormInput
            v-if="shown(enteringType, 'vendor')"
            v-model="recordVendor"
            :label="$t('asset-lifecycle-record-vendor')"
            :required="required(enteringType, 'vendor')"
          />
          <FormInput
            v-if="shown(enteringType, 'reference')"
            v-model="recordReference"
            :label="$t('asset-lifecycle-record-reference')"
            :placeholder="$t('asset-lifecycle-record-reference-placeholder')"
            :required="required(enteringType, 'reference')"
          />
          <DynamicAttributeForm
            v-model="recordAttributes"
            :schema="orderedSchema(enteringType)"
          />
        </template>
      </section>

      <template v-if="legacyRepair">
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
        <Button
          :loading="submitting"
          :disabled="!toStatus || blocked"
          @click="submitTransition"
        >
          {{ $t('asset-lifecycle-modal-submit') }}
        </Button>
      </div>
    </template>
  </Modal>
</template>
