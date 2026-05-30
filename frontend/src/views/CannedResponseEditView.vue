<script setup lang="ts">
/**
 * Full-page editor for a canned response. Used for both create
 * (route param missing) and edit (route param numeric). Right pane
 * renders a live preview against fixed placeholder data so the
 * admin can sanity-check variable usage before saving; left pane
 * hosts the title input + chip-aware body editor.
 *
 * Starter pre-fill: when navigated with `?starter=<slug>` (set by
 * StarterCatalogModal), the editor hydrates from the catalog
 * before the admin types anything. Nothing persists until Save.
 */
import { ref, computed, onMounted, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BackButton from '@/components/common/BackButton.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import TemplateEditor from '@/components/cannedResponseComponents/TemplateEditor.vue';
import SamplePreview from '@/components/cannedResponseComponents/SamplePreview.vue';
import cannedResponsesService, {
  findUnknownVariables,
  type CannedResponseListItem,
} from '@/services/cannedResponsesService';
import { extractErrorMessage } from '@/utils/errors';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const route = useRoute();
const router = useRouter();
const queryCache = useQueryCache();

const CANNED_RESPONSES_KEY = ['canned-responses'] as const;

// Route param: numeric id for edit, "new" for create. We branch on
// "new" rather than absence-of-param because Vue Router treats the
// /new path as a static segment, not a missing param.
const routeId = computed<string>(() => String(route.params.id ?? ''));
const isCreating = computed(() => routeId.value === 'new' || routeId.value === '');
const editingId = computed<number | null>(() => {
  if (isCreating.value) return null;
  const n = Number(routeId.value);
  return Number.isFinite(n) ? n : null;
});

// Pull from the same Pinia Colada cache the list view populates,
// so a navigation list -> editor hits cache and renders instantly.
// Cold-load (direct URL hit) falls through to a network fetch.
const listQuery = useQuery({
  key: CANNED_RESPONSES_KEY,
  query: () => cannedResponsesService.list(),
});
const existing = computed<CannedResponseListItem | null>(() => {
  if (editingId.value === null) return null;
  const rows = Array.isArray(listQuery.data.value) ? listQuery.data.value : [];
  return rows.find((r) => r.id === editingId.value) ?? null;
});

const form = ref({ title: '', body: '' });
const hasHydrated = ref(false);
const isSaving = ref(false);
const formError = ref('');
const successMessage = ref('');

// Hydrate the form from the matching row once the list resolves.
// Watch covers the case where the user navigated direct via URL
// (list cache empty -> fetch -> row arrives).
watch(
  existing,
  (row) => {
    if (!row || hasHydrated.value) return;
    form.value = { title: row.title, body: row.body };
    hasHydrated.value = true;
  },
  { immediate: true },
);

// Starter pre-fill (create path only). Fetched lazily so the
// network round-trip happens only when ?starter= is present.
onMounted(async () => {
  if (!isCreating.value) return;
  const slug = route.query.starter as string | undefined;
  if (!slug) return;
  try {
    const catalog = await cannedResponsesService.getStarterCatalog();
    const match = catalog.find((s) => s.slug === slug);
    if (match) {
      form.value = { title: match.title, body: match.body };
    }
  } catch {
    // Silent on starter fetch failure; admin can still author by
    // hand. Surfacing an error here would be misleading because
    // the page itself is fine.
  }
});

const unknownVariables = computed<string[]>(() => findUnknownVariables(form.value.body));

const pageTitle = computed(() =>
  isCreating.value
    ? t('admin-canned-responses-create-title')
    : t('admin-canned-responses-edit-title'),
);
const submitLabel = computed(() =>
  isCreating.value
    ? t('admin-canned-responses-create-submit')
    : t('admin-canned-responses-save'),
);

// Cold-load skeleton condition: only show while the edit form is
// waiting for the list cache to hydrate the row. Create-mode
// renders the editor immediately, no skeleton needed.
const isHydrating = computed(
  () =>
    !isCreating.value &&
    !hasHydrated.value &&
    listQuery.status.value === 'pending' &&
    listQuery.data.value === undefined,
);

const notFound = computed(
  () =>
    !isCreating.value &&
    listQuery.status.value === 'success' &&
    existing.value === null,
);

async function submit(): Promise<void> {
  const title = form.value.title.trim();
  const body = form.value.body.trim();
  if (!title) {
    formError.value = t('admin-canned-responses-error-title-required');
    return;
  }
  if (!body) {
    formError.value = t('admin-canned-responses-error-body-required');
    return;
  }
  if (unknownVariables.value.length > 0) {
    formError.value = t('admin-canned-responses-error-unknown-variables', {
      names: unknownVariables.value.join(', '),
    });
    return;
  }
  isSaving.value = true;
  formError.value = '';
  try {
    if (isCreating.value) {
      await cannedResponsesService.create({ title, body });
    } else if (editingId.value !== null) {
      await cannedResponsesService.update(editingId.value, { title, body });
    }
    await queryCache.invalidateQueries({ key: CANNED_RESPONSES_KEY });
    successMessage.value = isCreating.value
      ? t('admin-canned-responses-success-created')
      : t('admin-canned-responses-success-updated');
    setTimeout(() => router.push({ name: 'admin-canned-responses' }), 600);
  } catch (error) {
    formError.value = extractErrorMessage(error, t('admin-canned-responses-error-save'));
  } finally {
    isSaving.value = false;
  }
}

function goBack(): void {
  router.push({ name: 'admin-canned-responses' });
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <!-- Heading with back link. Uses the shared BackButton primitive
           so the chevron + visible label match every other detail view
           (AssetView, TicketView, etc.) and "previous route or admin
           list" navigation behaviour is identical. -->
      <div class="flex flex-col gap-1">
        <BackButton
          :fallback-route="'/admin/canned-responses'"
          :label="t('admin-canned-responses-edit-back-label')"
          compact
        />
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ pageTitle }}</h1>
      </div>

      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />
      <AlertMessage v-if="formError" type="error" :message="formError" />

      <!-- "Row not found" guard: an admin URL-pasted into an id
           that doesn't exist (or got deleted in another tab). -->
      <AlertMessage
        v-if="notFound"
        type="error"
        :message="$t('admin-canned-responses-edit-not-found')"
      />

      <Skeleton
        v-if="isHydrating"
        :label="$t('admin-canned-responses-loading')"
        class="grid grid-cols-1 lg:grid-cols-2 gap-4"
      >
        <div class="flex flex-col gap-3">
          <SkeletonBar class="h-10 w-full" />
          <SkeletonBar class="h-64 w-full" />
        </div>
        <SkeletonBar class="h-64 w-full" />
      </Skeleton>

      <form
        v-else-if="!notFound"
        class="grid grid-cols-1 lg:grid-cols-2 gap-4"
        @submit.prevent="submit"
      >
        <!-- Left: title + body editor -->
        <div class="flex flex-col gap-3">
          <FormInput
            v-model="form.title"
            :label="$t('admin-canned-responses-field-title')"
            :placeholder="$t('admin-canned-responses-field-title-placeholder')"
            required
          />
          <div class="flex flex-col gap-1">
            <label class="text-sm font-medium text-primary">
              {{ $t('admin-canned-responses-field-body') }}
            </label>
            <TemplateEditor
              v-model="form.body"
              :placeholder="$t('admin-canned-responses-field-body-placeholder')"
            />
            <p
              v-if="unknownVariables.length > 0"
              class="text-xs text-status-warning mt-1"
            >
              {{
                $t('admin-canned-responses-warn-unknown-variables', {
                  names: unknownVariables.join(', '),
                })
              }}
            </p>
          </div>
        </div>

        <!-- Right: sample-data preview -->
        <SamplePreview :body="form.body" />

        <!-- Save / cancel sit below the two-column grid -->
        <div
          class="col-span-1 lg:col-span-2 flex justify-end gap-2 border-t border-default pt-3"
        >
          <Button variant="secondary" type="button" @click="goBack">
            {{ $t('admin-canned-responses-cancel') }}
          </Button>
          <Button type="submit" :loading="isSaving">
            {{ submitLabel }}
          </Button>
        </div>
      </form>
    </div>
  </div>
</template>
