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
import { onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';

import AlertMessage from '@/components/common/AlertMessage.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import EmptyState from '@/components/common/EmptyState.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import {
  emailSuppressionsService,
  type EmailSuppression,
} from '@/services/emailSuppressionsService';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

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
        errorMessage.value = e.message || t('admin-suppressions-error-load');
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
        errorMessage.value = e.message || t('admin-suppressions-error-load-more');
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
        addError.value = e.message || t('admin-suppressions-error-add');
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
        errorMessage.value = e.message || t('admin-suppressions-error-remove');
    }
}

const reasonTone: Record<string, string> = {
    hard_bounce: 'bg-red-500/10 text-red-700 dark:text-red-400',
    manual: 'bg-blue-500/10 text-blue-700 dark:text-blue-400',
};

function toneFor(reason: string): string {
    return reasonTone[reason] ?? 'bg-default text-secondary';
}

function reasonLabel(reason: string): string {
    if (reason === 'hard_bounce') return t('admin-suppressions-reason-hard-bounce');
    if (reason === 'manual') return t('admin-suppressions-reason-manual');
    return reason.replace('_', ' ');
}

function formatDateTime(iso: string): string {
    return new Date(iso).toLocaleString();
}

onMounted(loadFirstPage);
</script>

<template>
    <div class="flex flex-col gap-6 p-6">
        <header class="flex flex-col gap-2">
            <h1 class="text-2xl font-semibold">{{ $t('admin-suppressions-title') }}</h1>
            <p class="text-sm text-secondary">
                {{ $t('admin-suppressions-description') }}
            </p>
        </header>

        <section class="rounded border border-default bg-surface p-3 inline-flex items-baseline gap-2 self-start">
            <span class="text-2xl font-semibold">{{ total }}</span>
            <span class="text-xs text-secondary uppercase tracking-wide">
                {{ total === 1 ? $t('admin-suppressions-count-singular') : $t('admin-suppressions-count-plural') }}
            </span>
        </section>

        <section class="flex flex-col gap-2 rounded border border-default bg-surface p-3">
            <h2 class="text-sm font-semibold text-primary">{{ $t('admin-suppressions-add-title') }}</h2>
            <form class="flex flex-col sm:flex-row gap-2" @submit.prevent="handleAdd">
                <input
                    v-model="newEmail"
                    type="email"
                    :placeholder="$t('admin-suppressions-add-email-placeholder')"
                    required
                    class="h-9 px-2 rounded border border-default bg-input text-primary text-sm flex-1"
                />
                <input
                    v-model="newNote"
                    type="text"
                    :placeholder="$t('admin-suppressions-add-note-placeholder')"
                    class="h-9 px-2 rounded border border-default bg-input text-primary text-sm flex-1"
                />
                <button
                    type="submit"
                    class="h-9 px-4 rounded bg-accent text-white text-sm font-medium disabled:opacity-50"
                    :disabled="isAdding || !newEmail.trim()"
                >
                    {{ isAdding ? $t('admin-suppressions-adding') : $t('admin-suppressions-add') }}
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
            :title="$t('admin-suppressions-empty-title')"
            :description="$t('admin-suppressions-empty-description')"
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
                    {{ reasonLabel(row.reason) }}
                </span>
                <span class="text-sm font-mono text-primary flex-1 truncate" :title="row.email">
                    {{ row.email }}
                </span>
                <span
                    v-if="row.bounce_count > 1"
                    class="text-xs text-secondary whitespace-nowrap"
                    :title="t('admin-suppressions-bounce-count-title', { count: row.bounce_count })"
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
                    {{ $t('admin-suppressions-remove') }}
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
                {{ isLoadingMore ? $t('admin-suppressions-loading-more') : $t('admin-suppressions-load-more') }}
            </button>
        </div>

        <ConfirmModal
            :show="showRemoveConfirm"
            variant="danger"
            :title="$t('admin-suppressions-confirm-title')"
            :message="$t('admin-suppressions-confirm-message')"
            :confirm-label="$t('admin-suppressions-remove')"
            :cancel-label="$t('admin-suppressions-confirm-keep')"
            @confirm="confirmRemove"
            @close="showRemoveConfirm = false"
        />
    </div>
</template>
