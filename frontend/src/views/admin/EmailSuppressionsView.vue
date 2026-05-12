<script setup lang="ts">
/**
 * Admin view for the email suppression list (J Pass 2.2b).
 *
 * Two flows:
 *   - Add an address manually (compliance / right-to-be-forgotten /
 *     observed complaints). Falls through to a `note` that lands
 *     on the row's diagnostic.
 *   - Remove an address (false-positive recovery — a hard bounce
 *     auto-suppressed a recipient that actually still works).
 *
 * Reads also serve as the audit trail: bounce_count and last_seen_at
 * show whether an address is chronically failing or one-off.
 */
import { computed, onMounted, ref } from 'vue';

import AlertMessage from '@/components/common/AlertMessage.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import {
  emailSuppressionsService,
  type EmailSuppression,
} from '@/services/emailSuppressionsService';

const rows = ref<EmailSuppression[]>([]);
const total = ref(0);
const nextCursor = ref<string | null>(null);

const isLoading = ref(false);
const isLoadingMore = ref(false);
const errorMessage = ref('');

const newEmail = ref('');
const newNote = ref('');
const isAdding = ref(false);
const addError = ref('');

const pendingRemove = ref<string | null>(null);
const showRemoveConfirm = ref(false);

async function loadFirstPage() {
    isLoading.value = true;
    errorMessage.value = '';
    try {
        const page = await emailSuppressionsService.list({ limit: 50 });
        rows.value = page.rows;
        total.value = page.total;
        nextCursor.value = page.next_cursor;
    } catch (err) {
        const e = err as { message?: string };
        errorMessage.value = e.message || 'Failed to load suppressions';
    } finally {
        isLoading.value = false;
    }
}

async function loadMore() {
    if (!nextCursor.value || isLoadingMore.value) return;
    isLoadingMore.value = true;
    try {
        const page = await emailSuppressionsService.list({
            before: nextCursor.value,
            limit: 50,
        });
        rows.value = [...rows.value, ...page.rows];
        total.value = page.total;
        nextCursor.value = page.next_cursor;
    } catch (err) {
        const e = err as { message?: string };
        errorMessage.value = e.message || 'Failed to load more';
    } finally {
        isLoadingMore.value = false;
    }
}

async function handleAdd() {
    const email = newEmail.value.trim();
    if (!email) return;
    isAdding.value = true;
    addError.value = '';
    try {
        await emailSuppressionsService.add(email, newNote.value.trim() || undefined);
        newEmail.value = '';
        newNote.value = '';
        await loadFirstPage();
    } catch (err) {
        const e = err as { message?: string };
        addError.value = e.message || 'Failed to add suppression';
    } finally {
        isAdding.value = false;
    }
}

function startRemove(email: string) {
    pendingRemove.value = email;
    showRemoveConfirm.value = true;
}

async function confirmRemove() {
    const email = pendingRemove.value;
    showRemoveConfirm.value = false;
    pendingRemove.value = null;
    if (!email) return;
    try {
        await emailSuppressionsService.remove(email);
        await loadFirstPage();
    } catch (err) {
        const e = err as { message?: string };
        errorMessage.value = e.message || 'Failed to remove';
    }
}

const reasonTone: Record<string, string> = {
    hard_bounce: 'bg-red-500/10 text-red-700 dark:text-red-400',
    manual: 'bg-blue-500/10 text-blue-700 dark:text-blue-400',
};

function toneFor(reason: string): string {
    return reasonTone[reason] ?? 'bg-default text-secondary';
}

function formatDateTime(iso: string): string {
    return new Date(iso).toLocaleString();
}

onMounted(loadFirstPage);
</script>

<template>
    <div class="flex flex-col gap-6 p-6">
        <header class="flex flex-col gap-2">
            <h1 class="text-2xl font-semibold">Email suppression list</h1>
            <p class="text-sm text-secondary">
                Addresses that we won't attempt to deliver to. Hard bounces
                (5xx SMTP / 5.x.x enhanced status) land here automatically;
                add manually for compliance or complaint-driven blocks. Soft
                bounces (4xx, transient) never auto-suppress.
            </p>
        </header>

        <section class="rounded border border-default bg-surface p-3 inline-flex items-baseline gap-2 self-start">
            <span class="text-2xl font-semibold">{{ total }}</span>
            <span class="text-xs text-secondary uppercase tracking-wide">
                {{ total === 1 ? 'suppression' : 'suppressions' }}
            </span>
        </section>

        <section class="flex flex-col gap-2 rounded border border-default bg-surface p-3">
            <h2 class="text-sm font-semibold text-primary">Add a suppression</h2>
            <form class="flex flex-col sm:flex-row gap-2" @submit.prevent="handleAdd">
                <input
                    v-model="newEmail"
                    type="email"
                    placeholder="user@example.com"
                    required
                    class="h-9 px-2 rounded border border-default bg-input text-primary text-sm flex-1"
                />
                <input
                    v-model="newNote"
                    type="text"
                    placeholder="Optional note (compliance request, etc.)"
                    class="h-9 px-2 rounded border border-default bg-input text-primary text-sm flex-1"
                />
                <button
                    type="submit"
                    class="h-9 px-4 rounded bg-accent text-white text-sm font-medium disabled:opacity-50"
                    :disabled="isAdding || !newEmail.trim()"
                >
                    {{ isAdding ? 'Adding…' : 'Add' }}
                </button>
            </form>
            <p v-if="addError" class="text-xs text-status-error">{{ addError }}</p>
        </section>

        <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

        <div v-if="isLoading" class="py-12 flex justify-center">
            <LoadingSpinner />
        </div>

        <EmptyState
            v-else-if="rows.length === 0"
            icon="inbox"
            title="No suppressions"
            description="Hard-bounced recipients and manually-added addresses will appear here."
        />

        <ul v-else class="flex flex-col gap-1">
            <li
                v-for="row in rows"
                :key="row.email"
                class="rounded border border-default bg-surface px-3 py-2 flex items-center gap-3"
            >
                <span
                    class="text-[10px] font-semibold uppercase tracking-wide px-2 py-0.5 rounded"
                    :class="toneFor(row.reason)"
                >
                    {{ row.reason.replace('_', ' ') }}
                </span>
                <span class="text-sm font-mono text-primary flex-1 truncate" :title="row.email">
                    {{ row.email }}
                </span>
                <span
                    v-if="row.bounce_count > 1"
                    class="text-xs text-secondary whitespace-nowrap"
                    :title="`Bounced ${row.bounce_count} times`"
                >
                    {{ row.bounce_count }}×
                </span>
                <span class="text-xs text-secondary whitespace-nowrap">
                    {{ formatDateTime(row.last_seen_at) }}
                </span>
                <button
                    type="button"
                    class="text-xs px-2 py-1 rounded border border-default hover:bg-hover"
                    @click="startRemove(row.email)"
                >
                    Remove
                </button>
            </li>
        </ul>

        <div v-if="nextCursor" class="flex justify-center pt-2">
            <button
                type="button"
                class="h-9 px-4 rounded border border-default text-sm hover:bg-hover disabled:opacity-50"
                :disabled="isLoadingMore"
                @click="loadMore"
            >
                {{ isLoadingMore ? 'Loading…' : 'Load more' }}
            </button>
        </div>

        <ConfirmModal
            :show="showRemoveConfirm"
            variant="danger"
            title="Remove from suppression list?"
            message="Future sends to this address will be attempted normally. If the original failure was a hard bounce, they'll likely fail and re-suppress."
            confirm-label="Remove"
            cancel-label="Keep suppressed"
            @confirm="confirmRemove"
            @close="showRemoveConfirm = false"
        />
    </div>
</template>
