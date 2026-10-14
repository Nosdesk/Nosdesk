<script setup lang="ts">
/**
 * The asset page's current state, above the details: the status, who has
 * the asset when it's out on loan and when it's due back, and the actions
 * that change either (Loan out, Return, Change status). The Status history
 * and Loans cards below keep the record of past changes.
 */
import { computed, ref } from 'vue';
import { RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery } from '@pinia/colada';
import Button from '@/components/common/Button.vue';
import StatusPill from '@/components/common/StatusPill.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import AssetStatusBadge from '@/components/assets/AssetStatusBadge.vue';
import AssetLoanIssueModal from '@/components/assets/AssetLoanIssueModal.vue';
import AssetLoanReturnModal from '@/components/assets/AssetLoanReturnModal.vue';
import AssetStatusChangeModal from '@/components/assets/AssetStatusChangeModal.vue';
import {
  assetLifecycleKeys,
  assetLifecycleService,
} from '@nosdesk/core/services/assetLifecycleService';
import { useAssetLoans } from '@/composables/useAssetLoans';
import { useSyncActions } from '@/composables/useSyncActions';
import { useUsersDirectory } from '@/composables/useUsersDirectory';
import { formatCompactDate } from '@nosdesk/core/utils/dateUtils';
import { loanDue } from './loanDue';

const props = defineProps<{
  assetId: number;
  status: string;
  canEdit?: boolean;
}>();

const emit = defineEmits<{
  /** A loan was issued or returned; the asset's status moved with it. */
  (e: 'loan-changed'): void;
  (e: 'transitioned', toStatus: string): void;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const { getUserHandle } = useUsersDirectory();

const { activeLoan, invalidate: invalidateLoans } = useAssetLoans(() => props.assetId);

useSyncActions(
  (actions) => {
    if (actions.some((a) => (a.data as { asset_id?: number }).asset_id === props.assetId)) {
      void invalidateLoans();
    }
  },
  { aggregates: ['asset_loan'], debounceMs: 250 },
);

// Shares the Status history card's query: the newest event says since when
// the asset has had its current status.
const lifecycleQuery = useQuery({
  key: () => assetLifecycleKeys.forAsset(props.assetId),
  query: () => assetLifecycleService.list(props.assetId),
});
const since = computed(() => {
  const latest = lifecycleQuery.data.value?.[0];
  if (!latest || latest.to_status !== props.status) return null;
  return formatCompactDate(latest.occurred_at);
});

const isTerminal = computed(() => props.status === 'disposed');
// Only in-service or in-stock assets with no open loan can be loaned out.
const canLoan = computed(
  () => !activeLoan.value && (props.status === 'in_service' || props.status === 'in_stock'),
);
// On loan is owned by the loan: returning it is what moves the status on.
const canChangeStatus = computed(() => !isTerminal.value && props.status !== 'on_loan');

const borrowerUuid = computed(() => activeLoan.value?.borrower_user_uuid ?? null);
const borrowerName = computed(() => {
  if (!activeLoan.value) return '';
  if (!borrowerUuid.value) return t('asset-loan-deleted-borrower');
  return getUserHandle(borrowerUuid.value).user.value?.name ?? t('asset-loan-unknown-borrower');
});
const due = computed(() => (activeLoan.value ? loanDue(activeLoan.value, t) : null));
const dueTitle = computed(() =>
  activeLoan.value?.due_back
    ? t('asset-loan-due-on', { date: formatCompactDate(activeLoan.value.due_back) })
    : undefined,
);

const showIssue = ref(false);
const showReturn = ref(false);
const showStatusChange = ref(false);

async function onLoanChanged() {
  showIssue.value = false;
  showReturn.value = false;
  await invalidateLoans();
  emit('loan-changed');
}

function onTransitioned(toStatus: string) {
  showStatusChange.value = false;
  emit('transitioned', toStatus);
}
</script>

<template>
  <section
    class="bg-surface rounded-xl border border-default px-4 py-3 flex flex-wrap items-center gap-x-4 gap-y-3"
    :aria-label="$t('asset-state-label')"
  >
    <div class="flex flex-wrap items-center gap-x-3 gap-y-2 min-w-0 flex-1">
      <!-- The state and its alert read together: "On loan · 16 days overdue". -->
      <div class="flex items-center gap-2">
        <AssetStatusBadge :status="status" size="md" />
        <StatusPill v-if="due" :label="due.label" :tone="due.tone" size="sm" :title="dueTitle" />
      </div>

      <!-- Who has it. Shown whenever a loan is open, so an asset marked
           lost while out still names the borrower. -->
      <div v-if="activeLoan" class="flex items-center gap-2 min-w-0">
        <UserAvatar
          v-if="borrowerUuid"
          :uuid="borrowerUuid"
          size="sm"
          :show-name="false"
          :clickable="false"
          class="shrink-0"
        />
        <div class="flex flex-col min-w-0">
          <RouterLink
            v-if="borrowerUuid"
            :to="`/users/${borrowerUuid}`"
            class="text-sm font-medium text-primary truncate leading-tight hover:underline"
          >{{ borrowerName }}</RouterLink>
          <span v-else class="text-sm font-medium text-secondary truncate leading-tight">{{ borrowerName }}</span>
          <span class="text-xs text-tertiary truncate leading-tight">
            {{ $t('asset-loan-fact-loaned') }} {{ formatCompactDate(activeLoan.loaned_at) }}
          </span>
        </div>
      </div>
      <span v-else-if="isTerminal" class="text-xs text-tertiary">{{ $t('asset-lifecycle-disposed-final') }}</span>
      <span v-else-if="since" class="text-xs text-tertiary">{{ $t('asset-state-since', { date: since }) }}</span>

      <slot name="meta" />
    </div>

    <!-- Below sm the actions take their own row rather than centring
         against the wrapped state block. -->
    <div
      v-if="canEdit && (activeLoan || canLoan || canChangeStatus)"
      class="flex flex-wrap items-center gap-2 basis-full sm:basis-auto"
    >
      <Button v-if="activeLoan" size="sm" variant="secondary" icon="check" @click="showReturn = true">
        {{ $t('asset-loan-return') }}
      </Button>
      <Button v-else-if="canLoan" size="sm" icon="userPlus" @click="showIssue = true">
        {{ $t('asset-loan-loan-out') }}
      </Button>
      <Button v-if="canChangeStatus" size="sm" variant="secondary" icon="refresh" @click="showStatusChange = true">
        {{ $t('asset-lifecycle-change-status') }}
      </Button>
    </div>

    <template v-if="canEdit">
      <AssetLoanIssueModal
        :show="showIssue"
        :asset-id="assetId"
        @close="showIssue = false"
        @issued="onLoanChanged"
      />
      <AssetLoanReturnModal
        :show="showReturn"
        :asset-id="assetId"
        :loan="activeLoan"
        :borrower-name="borrowerName"
        @close="showReturn = false"
        @returned="onLoanChanged"
      />
      <AssetStatusChangeModal
        :show="showStatusChange"
        :asset-id="assetId"
        :current-status="status"
        :has-active-loan="!!activeLoan"
        @close="showStatusChange = false"
        @transitioned="onTransitioned"
      />
    </template>
  </section>
</template>
