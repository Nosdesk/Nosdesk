<script setup lang="ts">
import { computed, ref } from 'vue';
import { RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import Button from '@/components/common/Button.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import DatePicker from '@/components/common/DatePicker.vue';
import Icon from '@/components/common/Icon.vue';
import StatusPill from '@/components/common/StatusPill.vue';
import type { StatusPillTone } from '@/components/common/statusPillTone';
import Modal from '@/components/Modal.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import UserSelectionModal from '@/components/UserSelectionModal.vue';
import TicketPickerModal from '@/components/ticketComponents/TicketPickerModal.vue';
import { assetLoanKeys, assetLoanService } from '@/services/assetLoanService';
import { useSyncActions } from '@/composables/useSyncActions';
import { useUsersDirectory } from '@/composables/useUsersDirectory';
import { formatCompactDate, formatRelativeTime } from '@/utils/dateUtils';
import type { AssetLoan } from '@nosdesk/core/types/asset';

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
  tone: StatusPillTone;
}

/** Due-date pill for the active loan, mapped straight to a StatusPill tone.
 * `null` when the loan is open-ended (no due date). */
const activeDue = computed<DueInfo | null>(() => {
  const loan = activeLoan.value;
  if (!loan?.due_back) return null;
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const due = new Date(`${loan.due_back}T00:00:00`);
  const days = Math.round((due.getTime() - today.getTime()) / 86_400_000);
  if (days < 0) return { label: t('asset-loan-due-overdue'), tone: 'critical' };
  if (days === 0) return { label: t('asset-loan-due-today'), tone: 'caution' };
  if (days <= 2) return { label: t('asset-loan-due-soon', { days }), tone: 'caution' };
  return { label: t('asset-loan-due-on', { date: formatCompactDate(loan.due_back) }), tone: 'neutral' };
});

const today = new Date().toISOString().slice(0, 10);

// ---- Issue flow -----------------------------------------------------

const showIssue = ref(false);
const showBorrowerPicker = ref(false);
const borrower = ref<{ uuid: string; name: string } | null>(null);
const loanedOn = ref(today);
const dueBack = ref('');
const linkedTicket = ref<{ id: number; title: string } | null>(null);
const showTicketPicker = ref(false);
const notes = ref('');
const issueError = ref('');
const submitting = ref(false);

function openIssue() {
  borrower.value = null;
  loanedOn.value = today;
  dueBack.value = '';
  linkedTicket.value = null;
  notes.value = '';
  issueError.value = '';
  showIssue.value = true;
}

function onSelectBorrower(user: { uuid: string; name: string }) {
  showBorrowerPicker.value = false;
  if (user.uuid) borrower.value = { uuid: user.uuid, name: user.name };
}

function onSelectTicket(ticket: { id: number; title: string }) {
  linkedTicket.value = ticket;
}

async function submitIssue() {
  if (!borrower.value) return;
  submitting.value = true;
  issueError.value = '';
  try {
    await assetLoanService.issue(props.assetId, {
      borrower_user_uuid: borrower.value.uuid,
      loaned_at: loanedOn.value || null,
      due_back: dueBack.value || null,
      ticket_id: linkedTicket.value?.id ?? null,
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
    <!-- Header: description + loan-out action (hidden once a loan is active) -->
    <div v-if="!activeLoan" class="flex items-center justify-between gap-3 flex-wrap">
      <p class="text-xs text-tertiary">{{ $t('asset-loan-description') }}</p>
      <Button v-if="canEdit && canLoan" size="sm" icon="userPlus" @click="openIssue">
        {{ $t('asset-loan-loan-out') }}
      </Button>
    </div>

    <p v-if="loadError" class="text-xs text-status-error">{{ loadError }}</p>

    <!-- Active loan: one vertically-centered identity row (avatar · borrower+meta ·
         due · action), with notes dropped beneath and aligned to the name. -->
    <div
      v-if="activeLoan"
      class="rounded-lg border border-default bg-surface-alt p-3 flex flex-col gap-2.5"
    >
      <div class="flex items-center gap-3">
        <UserAvatar
          :uuid="activeLoan.borrower_user_uuid"
          size="md"
          :show-name="false"
          :clickable="false"
          class="shrink-0"
        />
        <div class="min-w-0 flex-1">
          <p class="text-sm font-semibold text-primary truncate leading-tight">
            {{ borrowerName(activeLoan) }}
          </p>
          <div class="flex items-center gap-x-1.5 text-xs text-tertiary leading-tight mt-0.5 overflow-hidden">
            <span class="truncate">{{ $t('asset-loan-fact-loaned') }} {{ formatCompactDate(activeLoan.loaned_at) }}</span>
            <span aria-hidden="true" class="shrink-0">·</span>
            <span class="shrink-0">{{ relative(activeLoan.loaned_at) }}</span>
            <template v-if="activeLoan.ticket_id">
              <span aria-hidden="true" class="shrink-0">·</span>
              <RouterLink
                :to="`/tickets/${activeLoan.ticket_id}`"
                class="shrink-0 text-accent hover:underline"
              >{{ $t('asset-loan-ticket', { id: activeLoan.ticket_id }) }}</RouterLink>
            </template>
          </div>
        </div>
        <StatusPill
          v-if="activeDue"
          :label="activeDue.label"
          :tone="activeDue.tone"
          size="sm"
          class="shrink-0"
        />
        <Button
          v-if="canEdit"
          size="sm"
          variant="secondary"
          icon="check"
          class="shrink-0"
          @click="openReturn"
        >
          {{ $t('asset-loan-return') }}
        </Button>
      </div>

      <!-- Notes, aligned to the borrower name (md avatar 2rem + gap-3 0.75rem) -->
      <p
        v-if="activeLoan.notes"
        class="text-sm text-secondary whitespace-pre-line break-words pl-11"
      >
        {{ activeLoan.notes }}
      </p>
    </div>

    <!-- Not loanable (no active loan, wrong status) -->
    <div
      v-else-if="!canLoan && !isFirstLoad"
      class="rounded-lg border border-dashed border-default bg-surface-alt p-3 text-xs text-tertiary"
    >
      {{ $t('asset-loan-not-loanable') }}
    </div>

    <!-- Empty: loanable, no active loan, no history -->
    <div
      v-else-if="canLoan && pastLoans.length === 0 && !isFirstLoad"
      class="rounded-lg border border-dashed border-default bg-surface-alt p-4 flex items-start gap-3"
    >
      <Icon name="calendar" class="text-tertiary flex-shrink-0 mt-0.5" />
      <div class="min-w-0">
        <p class="text-sm font-medium text-primary">{{ $t('asset-loan-empty-title') }}</p>
        <p class="text-xs text-tertiary mt-1">{{ $t('asset-loan-empty-description') }}</p>
      </div>
    </div>

    <!-- History -->
    <div v-if="pastLoans.length > 0" class="flex flex-col gap-1.5">
      <p class="text-xs font-medium text-tertiary uppercase tracking-wide">
        {{ $t('asset-loan-history') }}
      </p>
      <div class="flex flex-col">
        <div
          v-for="loan in pastLoans"
          :key="loan.id"
          class="flex items-center gap-2.5 py-2 border-t border-subtle first:border-t-0"
        >
          <UserAvatar
            :uuid="loan.borrower_user_uuid"
            size="xs"
            :show-name="false"
            :clickable="false"
            class="shrink-0"
          />
          <div class="min-w-0 flex-1">
            <p class="text-sm text-primary truncate">{{ borrowerName(loan) }}</p>
            <p class="text-xs text-tertiary truncate">{{ loanRange(loan) }}</p>
          </div>
          <span class="text-xs text-tertiary whitespace-nowrap shrink-0">
            {{ relative(loan.returned_at) }}
          </span>
        </div>
      </div>
    </div>

    <!-- Issue modal -->
    <Modal :show="showIssue" :title="$t('asset-loan-issue-title')" size="md" @close="showIssue = false">
      <div class="flex flex-col gap-4">
        <!-- Borrower selector -->
        <div class="flex flex-col gap-1.5">
          <span class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('asset-loan-borrower') }}</span>
          <button
            type="button"
            class="flex items-center gap-2 rounded-lg border border-default bg-surface px-3 py-2 text-left hover:border-strong transition-colors"
            @click="showBorrowerPicker = true"
          >
            <template v-if="borrower">
              <UserAvatar :uuid="borrower.uuid" size="xs" :show-name="false" :clickable="false" class="shrink-0" />
              <span class="text-sm text-primary truncate">{{ borrower.name }}</span>
            </template>
            <template v-else>
              <Icon name="userPlus" class="w-4 h-4 text-tertiary shrink-0" />
              <span class="text-sm text-tertiary">{{ $t('asset-loan-select-borrower') }}</span>
            </template>
            <Icon name="chevronDown" class="ml-auto w-4 h-4 text-tertiary shrink-0" />
          </button>
        </div>

        <!-- Loan period -->
        <div class="flex flex-col gap-1.5">
          <span class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('asset-loan-period-label') }}</span>
          <DatePicker
            range
            v-model:start="loanedOn"
            v-model:end="dueBack"
            :aria-label="$t('asset-loan-period-label')"
            block
          />
          <p class="text-xs text-tertiary">{{ $t('asset-loan-period-hint') }}</p>
        </div>

        <!-- Linked ticket selector -->
        <div class="flex flex-col gap-1.5">
          <span class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('asset-loan-ticket-field') }}</span>
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="flex-1 min-w-0 flex items-center gap-2 rounded-lg border border-default bg-surface px-3 py-2 text-left hover:border-strong transition-colors"
              @click="showTicketPicker = true"
            >
              <template v-if="linkedTicket">
                <span class="text-xs font-mono text-tertiary shrink-0">#{{ linkedTicket.id }}</span>
                <span class="text-sm text-primary truncate">{{ linkedTicket.title || $t('ticket-picker-untitled') }}</span>
              </template>
              <template v-else>
                <Icon name="ticket" size="sm" class="text-tertiary shrink-0" />
                <span class="text-sm text-tertiary">{{ $t('asset-loan-ticket-link') }}</span>
              </template>
              <Icon v-if="!linkedTicket" name="chevronDown" class="ml-auto w-4 h-4 text-tertiary shrink-0" />
            </button>
            <button
              v-if="linkedTicket"
              type="button"
              class="p-2 text-tertiary hover:text-status-error hover:bg-status-error-muted rounded-lg transition-colors shrink-0"
              :title="$t('asset-loan-ticket-clear')"
              @click="linkedTicket = null"
            >
              <Icon name="close" size="sm" />
            </button>
          </div>
        </div>

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

    <!-- Ticket picker (search existing or create new), over the issue modal -->
    <TicketPickerModal
      :show="showTicketPicker"
      allow-create
      @close="showTicketPicker = false"
      @select="onSelectTicket"
    />
  </div>
</template>
