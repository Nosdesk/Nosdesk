<script setup lang="ts">
import { computed, ref } from 'vue';
import { RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import DatePicker from '@/components/common/DatePicker.vue';
import Icon from '@/components/common/Icon.vue';
import Modal from '@/components/Modal.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import UserSelectionModal from '@/components/UserSelectionModal.vue';
import { assetLoanKeys, assetLoanService } from '@/services/assetLoanService';
import { useSyncActions } from '@/composables/useSyncActions';
import { useUsersDirectory } from '@/composables/useUsersDirectory';
import { formatCompactDate, formatRelativeTime } from '@/utils/dateUtils';
import type { AssetLoan } from '@/types/asset';

const props = defineProps<{
  assetId: number;
  currentStatus: string;
  canEdit?: boolean;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const { getUserHandle } = useUsersDirectory();

const queryCache = useQueryCache();
const loansQuery = useQuery({
  key: () => assetLoanKeys.forAsset(props.assetId),
  query: () => assetLoanService.list(props.assetId),
});
const loans = computed<AssetLoan[]>(() =>
  Array.isArray(loansQuery.data.value) ? loansQuery.data.value : [],
);
const activeLoan = computed<AssetLoan | null>(
  () => loans.value.find((l) => !l.returned_at) ?? null,
);
const pastLoans = computed<AssetLoan[]>(() => loans.value.filter((l) => l.returned_at));
const isFirstLoad = computed(
  () => loansQuery.status.value === 'pending' && loansQuery.data.value === undefined,
);
const loadError = computed(() => (loansQuery.error.value ? t('asset-loan-load-error') : ''));

// Only in-service / in-stock assets with no active loan can be loaned out.
const canLoan = computed(
  () =>
    !activeLoan.value &&
    (props.currentStatus === 'in_service' || props.currentStatus === 'in_stock'),
);

function invalidate() {
  return queryCache.invalidateQueries({ key: assetLoanKeys.forAsset(props.assetId) });
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
  { aggregates: ['asset_loan'], debounceMs: 250 },
);

// ---- Display helpers ------------------------------------------------

function borrowerName(loan: AssetLoan): string {
  return getUserHandle(loan.borrower_user_uuid).user.value?.name ?? t('asset-loan-unknown-borrower');
}

function relative(date: string | null | undefined): string {
  return date ? formatRelativeTime(date, { addSuffix: true }) : '';
}

/** Range "loaned -> returned" for a finished loan, used in the history list. */
function loanRange(loan: AssetLoan): string {
  return t('asset-loan-range', {
    from: formatCompactDate(loan.loaned_at),
    to: loan.returned_at ? formatCompactDate(loan.returned_at) : '',
  });
}

interface DueInfo {
  label: string;
  tone: 'overdue' | 'soon' | 'normal';
}

/** Due-date pill/text for the active loan. `null` when the loan is
 * open-ended (no due date). Overdue and due-soon get a coloured pill; a
 * comfortable due date renders as quiet text. */
const activeDue = computed<DueInfo | null>(() => {
  const loan = activeLoan.value;
  if (!loan?.due_back) return null;
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const due = new Date(`${loan.due_back}T00:00:00`);
  const days = Math.round((due.getTime() - today.getTime()) / 86_400_000);
  if (days < 0) return { label: t('asset-loan-due-overdue'), tone: 'overdue' };
  if (days === 0) return { label: t('asset-loan-due-today'), tone: 'soon' };
  if (days <= 2) return { label: t('asset-loan-due-soon', { days }), tone: 'soon' };
  return { label: t('asset-loan-due-on', { date: formatCompactDate(loan.due_back) }), tone: 'normal' };
});

const today = new Date().toISOString().slice(0, 10);

// ---- Issue flow -----------------------------------------------------

const showIssue = ref(false);
const showBorrowerPicker = ref(false);
const borrower = ref<{ uuid: string; name: string } | null>(null);
const dueBack = ref('');
const ticketIdInput = ref('');
const notes = ref('');
const issueError = ref('');
const submitting = ref(false);

function openIssue() {
  borrower.value = null;
  dueBack.value = '';
  ticketIdInput.value = '';
  notes.value = '';
  issueError.value = '';
  showIssue.value = true;
}

function onSelectBorrower(user: { uuid: string; name: string }) {
  showBorrowerPicker.value = false;
  if (user.uuid) borrower.value = { uuid: user.uuid, name: user.name };
}

function parseTicketId(): number | null {
  const raw = ticketIdInput.value.trim();
  if (!raw) return null;
  const n = Number.parseInt(raw, 10);
  return Number.isFinite(n) && n > 0 ? n : null;
}

async function submitIssue() {
  if (!borrower.value) return;
  submitting.value = true;
  issueError.value = '';
  try {
    await assetLoanService.issue(props.assetId, {
      borrower_user_uuid: borrower.value.uuid,
      due_back: dueBack.value || null,
      ticket_id: parseTicketId(),
      notes: notes.value.trim() || null,
    });
    await invalidate();
    showIssue.value = false;
  } catch (e) {
    issueError.value = e instanceof Error ? e.message : t('asset-loan-failed');
  } finally {
    submitting.value = false;
  }
}

// ---- Return flow ----------------------------------------------------

const showReturn = ref(false);
const returnNotes = ref('');
const returnError = ref('');

function openReturn() {
  returnNotes.value = '';
  returnError.value = '';
  showReturn.value = true;
}

async function submitReturn() {
  const loan = activeLoan.value;
  if (!loan) return;
  submitting.value = true;
  returnError.value = '';
  try {
    await assetLoanService.returnLoan(props.assetId, loan.id, {
      notes: returnNotes.value.trim() || null,
    });
    await invalidate();
    showReturn.value = false;
  } catch (e) {
    returnError.value = e instanceof Error ? e.message : t('asset-loan-failed');
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <div class="flex items-center justify-between gap-3 flex-wrap">
      <p class="text-xs text-tertiary">{{ $t('asset-loan-description') }}</p>
      <Button v-if="canEdit && canLoan" size="sm" icon="userPlus" @click="openIssue">
        {{ $t('asset-loan-loan-out') }}
      </Button>
    </div>

    <p v-if="loadError" class="text-xs text-status-error">{{ loadError }}</p>

    <!-- Active loan -->
    <div
      v-if="activeLoan"
      class="rounded-lg border border-default bg-surface-alt p-3 flex flex-col gap-2"
    >
      <div class="flex items-start justify-between gap-3">
        <div class="flex items-center gap-2 min-w-0">
          <UserAvatar :uuid="activeLoan.borrower_user_uuid" size="sm" :clickable="false" />
          <div class="min-w-0">
            <p class="text-sm font-medium text-primary truncate">{{ borrowerName(activeLoan) }}</p>
            <p class="text-xs text-tertiary">
              {{ $t('asset-loan-loaned-relative', { when: relative(activeLoan.loaned_at) }) }}
            </p>
          </div>
        </div>
        <span
          v-if="activeDue"
          class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium whitespace-nowrap"
          :class="{
            'bg-status-error-muted text-status-error': activeDue.tone === 'overdue',
            'bg-status-warning-bg text-status-warning': activeDue.tone === 'soon',
            'text-tertiary': activeDue.tone === 'normal',
          }"
        >
          <Icon name="calendar" class="w-3 h-3" />
          {{ activeDue.label }}
        </span>
      </div>

      <p v-if="activeLoan.notes" class="text-xs text-secondary">{{ activeLoan.notes }}</p>

      <div class="flex items-center justify-between gap-2">
        <RouterLink
          v-if="activeLoan.ticket_id"
          :to="`/tickets/${activeLoan.ticket_id}`"
          class="inline-flex items-center px-1.5 py-0.5 rounded border border-default bg-surface text-accent text-xs hover:underline"
        >
          {{ $t('asset-loan-ticket', { id: activeLoan.ticket_id }) }}
        </RouterLink>
        <span v-else />
        <Button v-if="canEdit" size="sm" variant="secondary" icon="check" @click="openReturn">
          {{ $t('asset-loan-return') }}
        </Button>
      </div>
    </div>

    <!-- Not loanable (no active loan, wrong status) -->
    <div
      v-else-if="!canLoan && !isFirstLoad"
      class="rounded-lg border border-dashed border-default bg-surface-alt p-3 text-xs text-tertiary"
    >
      {{ $t('asset-loan-not-loanable') }}
    </div>

    <!-- History -->
    <div v-if="pastLoans.length > 0" class="flex flex-col gap-1">
      <p class="text-xs font-medium text-tertiary uppercase tracking-wide">
        {{ $t('asset-loan-history') }}
      </p>
      <div class="divide-y divide-default">
        <div
          v-for="loan in pastLoans"
          :key="loan.id"
          class="py-2 flex items-baseline justify-between gap-3"
        >
          <div class="min-w-0 flex items-baseline gap-1.5">
            <span class="text-sm text-primary truncate">{{ borrowerName(loan) }}</span>
            <span class="text-xs text-tertiary whitespace-nowrap">· {{ loanRange(loan) }}</span>
          </div>
          <span class="text-xs text-tertiary whitespace-nowrap">{{ relative(loan.returned_at) }}</span>
        </div>
      </div>
    </div>

    <!-- Empty: no active loan, loanable, no history -->
    <div
      v-else-if="!activeLoan && canLoan && !isFirstLoad"
      class="rounded-lg border border-dashed border-default bg-surface-alt p-4 flex items-start gap-3"
    >
      <Icon name="history" class="text-tertiary flex-shrink-0 mt-0.5" />
      <div class="min-w-0">
        <p class="text-sm font-medium text-primary">{{ $t('asset-loan-empty-title') }}</p>
        <p class="text-xs text-tertiary mt-1">{{ $t('asset-loan-empty-description') }}</p>
      </div>
    </div>

    <!-- Issue modal -->
    <Modal :show="showIssue" :title="$t('asset-loan-issue-title')" size="md" @close="showIssue = false">
      <div class="flex flex-col gap-4">
        <div class="flex flex-col gap-1.5">
          <span class="text-xs font-medium text-tertiary">{{ $t('asset-loan-borrower') }}</span>
          <button
            type="button"
            class="flex items-center gap-2 rounded-lg border border-default bg-surface px-3 py-2 text-left hover:bg-surface-hover transition-colors"
            @click="showBorrowerPicker = true"
          >
            <template v-if="borrower">
              <UserAvatar :uuid="borrower.uuid" size="xs" :clickable="false" />
              <span class="text-sm text-primary">{{ borrower.name }}</span>
            </template>
            <template v-else>
              <Icon name="userPlus" class="w-4 h-4 text-tertiary" />
              <span class="text-sm text-tertiary">{{ $t('asset-loan-select-borrower') }}</span>
            </template>
          </button>
        </div>

        <DatePicker v-model="dueBack" :label="$t('asset-loan-due-back-optional')" :min="today" />

        <FormInput
          v-model="ticketIdInput"
          :label="$t('asset-loan-ticket-field')"
          :placeholder="$t('asset-loan-ticket-field-placeholder')"
          inputmode="numeric"
        />

        <FormTextarea v-model="notes" :label="$t('asset-loan-notes')" :rows="2" />

        <p v-if="issueError" class="text-sm text-status-error">{{ issueError }}</p>
      </div>
      <template #footer>
        <div class="modal-actions">
          <Button variant="secondary" @click="showIssue = false">{{ $t('asset-loan-cancel') }}</Button>
          <Button :loading="submitting" :disabled="!borrower" @click="submitIssue">
            {{ $t('asset-loan-loan-out') }}
          </Button>
        </div>
      </template>
    </Modal>

    <!-- Return modal -->
    <Modal :show="showReturn" :title="$t('asset-loan-return-title')" size="md" @close="showReturn = false">
      <div class="flex flex-col gap-4">
        <p v-if="activeLoan" class="text-sm text-secondary">
          {{ $t('asset-loan-return-body', { name: borrowerName(activeLoan) }) }}
        </p>
        <FormTextarea v-model="returnNotes" :label="$t('asset-loan-return-notes')" :rows="2" />
        <p v-if="returnError" class="text-sm text-status-error">{{ returnError }}</p>
      </div>
      <template #footer>
        <div class="modal-actions">
          <Button variant="secondary" @click="showReturn = false">{{ $t('asset-loan-cancel') }}</Button>
          <Button :loading="submitting" @click="submitReturn">{{ $t('asset-loan-return') }}</Button>
        </div>
      </template>
    </Modal>

    <!-- Borrower picker, layered over the issue modal -->
    <UserSelectionModal
      :show="showBorrowerPicker"
      :current-user-id="borrower?.uuid ?? null"
      @close="showBorrowerPicker = false"
      @select-user="onSelectBorrower"
    />
  </div>
</template>
