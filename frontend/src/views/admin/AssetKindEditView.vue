<script setup lang="ts">
/**
 * Full-page editor for asset kinds. Used for both create
 * (route name `admin-asset-kinds-new`) and edit (`admin-asset
 * -kinds-edit`, /{id}). Form lives in its own route so the
 * upcoming schema-builder UI has the room it needs without
 * crowding the registry list.
 *
 * Slug is editable only at create time (the backend treats it
 * as immutable so existing asset rows keep resolving). The
 * attribute_schema field is a raw JSON textarea in this pass,
 * with a Prettify button as the interim usability nudge; the
 * typed schema-builder UI lands in the next commit.
 */
import { computed, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useQueryCache } from '@pinia/colada';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BackButton from '@/components/common/BackButton.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery';
import {
  assetKindsService,
  ASSET_KINDS_QUERY_KEY,
  ASSET_KIND_CATEGORIES,
  type AssetKind,
  type AssetKindCategory,
  type CreateAssetKindBody,
} from '@/services/assetKindsService';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@/stores/toast';
import { RouterLink } from 'vue-router';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const route = useRoute();
const router = useRouter();
const queryCache = useQueryCache();
const toast = useToastStore();

// "new" sentinel vs numeric id; matches the canned-responses
// pattern so URL semantics are identical across admin editors.
const routeId = computed<string>(() => String(route.params.id ?? ''));
const isCreating = computed(() => routeId.value === 'new' || routeId.value === '');
const editingId = computed<number | null>(() => {
  if (isCreating.value) return null;
  const n = Number(routeId.value);
  return Number.isFinite(n) ? n : null;
});

const { kinds, isFirstLoad } = useAssetKindsQuery();
const existing = computed<AssetKind | null>(() => {
  if (editingId.value === null) return null;
  return kinds.value.find((k) => k.id === editingId.value) ?? null;
});

interface FormState {
  slug: string;
  label: string;
  description: string;
  icon: string;
  sort_order: number;
  category: AssetKindCategory;
  attribute_schema: string;
}

const EMPTY_SCHEMA = '{\n  "type": "object",\n  "properties": {}\n}';

const form = ref<FormState>({
  slug: '',
  label: '',
  description: '',
  icon: '',
  sort_order: 100,
  category: 'generic',
  attribute_schema: EMPTY_SCHEMA,
});
const hasHydrated = ref(false);
const isSaving = ref(false);
const formError = ref('');

// Hydrate from the list cache once the matching row arrives. The
// immediate-watch handles the warm-cache case (paint without a
// flash); the watch-trigger handles the cold-cache case (URL
// directly opened by an admin, list query in flight).
watch(
  existing,
  (row) => {
    if (!row || hasHydrated.value) return;
    form.value = {
      slug: row.slug,
      label: row.label,
      description: row.description ?? '',
      icon: row.icon ?? '',
      sort_order: row.sort_order,
      category: row.category,
      attribute_schema: JSON.stringify(row.attribute_schema, null, 2),
    };
    hasHydrated.value = true;
  },
  { immediate: true },
);

// Slug auto-suggest from label (create-mode only; edit-mode locks
// the slug because the backend treats it as immutable so existing
// asset rows keep resolving). Only auto-fills while the slug field
// is empty or matches the previous derived value, so an admin who
// has typed a custom slug doesn't get it overwritten by further
// label edits.
let lastDerivedSlug = '';
function deriveSlug(label: string): string {
  return label
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9_\s-]+/g, '')
    .replace(/[\s-]+/g, '_')
    .slice(0, 64);
}
watch(
  () => form.value.label,
  (label) => {
    if (!isCreating.value) return;
    const next = deriveSlug(label);
    if (form.value.slug === '' || form.value.slug === lastDerivedSlug) {
      form.value.slug = next;
      lastDerivedSlug = next;
    }
  },
);

onMounted(() => {
  // create-mode starts with EMPTY_SCHEMA already set
});

// 409 conflict surface: pre-existing rows would fail validation
// under the new schema. The capture preserves the in-progress
// draft so the admin can review the sample, then Force-apply.
interface SchemaConflict {
  invalidCount: number;
  sample: Array<{ id: number; name: string; error: string }>;
}
const schemaConflict = ref<SchemaConflict | null>(null);

const pageTitle = computed(() =>
  isCreating.value
    ? t('admin-asset-kinds-create-title')
    : t('admin-asset-kinds-edit-title'),
);
const submitLabel = computed(() =>
  isCreating.value
    ? t('admin-asset-kinds-create-button')
    : t('admin-asset-kinds-save'),
);

const isHydrating = computed(
  () => !isCreating.value && !hasHydrated.value && isFirstLoad.value,
);

const notFound = computed(
  () =>
    !isCreating.value &&
    !isFirstLoad.value &&
    !hasHydrated.value &&
    existing.value === null,
);

function parseSchema(input: string): Record<string, unknown> {
  const trimmed = input.trim();
  if (!trimmed) return { type: 'object', properties: {} };
  return JSON.parse(trimmed);
}

function prettifySchema(): void {
  try {
    const parsed = parseSchema(form.value.attribute_schema);
    form.value.attribute_schema = JSON.stringify(parsed, null, 2);
    formError.value = '';
  } catch (e) {
    formError.value = t('admin-asset-kinds-error-bad-schema-json', {
      error: e instanceof Error ? e.message : String(e),
    });
  }
}

async function submit(force = false): Promise<void> {
  const slug = form.value.slug.trim();
  const label = form.value.label.trim();
  if (isCreating.value && !slug) {
    formError.value = t('admin-asset-kinds-error-slug-required');
    return;
  }
  if (!label) {
    formError.value = t('admin-asset-kinds-error-label-required');
    return;
  }
  let schema: Record<string, unknown>;
  try {
    schema = parseSchema(form.value.attribute_schema);
  } catch (e) {
    formError.value = t('admin-asset-kinds-error-bad-schema-json', {
      error: e instanceof Error ? e.message : String(e),
    });
    return;
  }

  isSaving.value = true;
  formError.value = '';
  if (!force) schemaConflict.value = null;
  try {
    if (isCreating.value) {
      const body: CreateAssetKindBody = {
        slug,
        label,
        description: form.value.description.trim() || null,
        icon: form.value.icon.trim() || null,
        sort_order: form.value.sort_order,
        category: form.value.category,
        attribute_schema: schema,
      };
      await assetKindsService.create(body);
      toast.success(t('admin-asset-kinds-created'));
    } else if (editingId.value !== null) {
      await assetKindsService.update(
        editingId.value,
        {
          label,
          description: form.value.description.trim() || null,
          icon: form.value.icon.trim() || null,
          sort_order: form.value.sort_order,
          category: form.value.category,
          attribute_schema: schema,
        },
        force ? { force: true } : undefined,
      );
      toast.success(t('admin-asset-kinds-saved'));
    }
    await queryCache.invalidateQueries({ key: ASSET_KINDS_QUERY_KEY });
    router.push({ name: 'admin-asset-kinds' });
  } catch (error) {
    // 409 with a structured body means existing rows would break
    // under the new schema. Capture the sample so the admin sees
    // what would fail and can Force-apply from the same form
    // without losing draft state.
    const err = error as {
      response?: {
        status?: number;
        data?: {
          error?: string;
          invalid_count?: number;
          sample?: SchemaConflict['sample'];
        };
      };
    };
    if (
      err?.response?.status === 409 &&
      err.response.data?.error === 'schema_invalidates_existing_assets' &&
      typeof err.response.data.invalid_count === 'number'
    ) {
      schemaConflict.value = {
        invalidCount: err.response.data.invalid_count,
        sample: err.response.data.sample ?? [],
      };
      return;
    }
    formError.value = extractErrorMessage(error, t('admin-asset-kinds-error-save'));
  } finally {
    isSaving.value = false;
  }
}

function cancel(): void {
  router.push({ name: 'admin-asset-kinds' });
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-5xl">
      <div class="flex flex-col gap-1">
        <BackButton
          :fallback-route="'/admin/asset-kinds'"
          :label="t('admin-asset-kinds-back-label')"
          compact
        />
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ pageTitle }}</h1>
      </div>

      <AlertMessage v-if="formError" type="error" :message="formError" />
      <AlertMessage
        v-if="notFound"
        type="error"
        :message="t('admin-asset-kinds-edit-not-found')"
      />

      <Skeleton
        v-if="isHydrating"
        :label="t('admin-asset-kinds-loading')"
        class="flex flex-col gap-3"
      >
        <SkeletonBar class="h-10 w-full" />
        <SkeletonBar class="h-10 w-full" />
        <SkeletonBar class="h-64 w-full" />
      </Skeleton>

      <form
        v-else-if="!notFound"
        class="flex flex-col gap-4"
        @submit.prevent="submit()"
      >
        <!-- Slug + Label row. Slug locks to read-only in edit
             mode because it's the immutable discriminator; existing
             asset rows would lose their resolution if we let it
             change. -->
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <FormInput
            v-model="form.slug"
            :label="t('admin-asset-kinds-field-slug')"
            :placeholder="t('admin-asset-kinds-field-slug-placeholder')"
            :disabled="!isCreating"
            :hint="
              isCreating
                ? t('admin-asset-kinds-field-slug-hint')
                : t('admin-asset-kinds-field-slug-locked')
            "
            required
          />
          <FormInput
            v-model="form.label"
            :label="t('admin-asset-kinds-field-label')"
            required
          />
        </div>

        <FormInput
          v-model="form.description"
          :label="t('admin-asset-kinds-field-description')"
        />

        <div class="grid grid-cols-1 sm:grid-cols-3 gap-3">
          <FormInput
            v-model="form.icon"
            :label="t('admin-asset-kinds-field-icon')"
            :hint="t('admin-asset-kinds-field-icon-hint')"
          />
          <!-- sort_order: number bound via v-model.number on a raw
               input because FormInput is string-typed; the integer
               edge case is small enough not to warrant a numeric
               variant of the primitive yet. -->
          <label class="flex flex-col gap-1 text-sm">
            <span class="font-medium text-primary">
              {{ t('admin-asset-kinds-field-sort-order') }}
            </span>
            <input
              v-model.number="form.sort_order"
              type="number"
              class="block w-full py-1.5 px-2 text-sm rounded-lg bg-surface-alt border border-default text-primary placeholder-tertiary transition-colors duration-200 hover:border-strong focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
            />
          </label>
          <label class="flex flex-col gap-1 text-sm">
            <span class="font-medium text-primary">
              {{ t('admin-asset-kinds-field-category') }}
            </span>
            <select
              v-model="form.category"
              class="block w-full py-1.5 px-2 text-sm rounded-lg bg-surface-alt border border-default text-primary transition-colors duration-200 hover:border-strong focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
            >
              <option v-for="c in ASSET_KIND_CATEGORIES" :key="c" :value="c">
                {{ t(`admin-asset-kinds-category-${c}`) }}
              </option>
            </select>
          </label>
        </div>

        <div class="flex flex-col gap-2">
          <div class="flex items-center justify-between gap-2">
            <span class="text-sm font-medium text-primary">
              {{ t('admin-asset-kinds-field-attribute-schema') }}
            </span>
            <Button
              variant="ghost"
              size="sm"
              icon="copy"
              type="button"
              @click="prettifySchema"
            >
              {{ t('admin-asset-kinds-prettify') }}
            </Button>
          </div>
          <FormTextarea
            v-model="form.attribute_schema"
            :rows="12"
            class="font-mono"
            :hint="t('admin-asset-kinds-field-attribute-schema-hint')"
          />
        </div>

        <!-- 409 schema-conflict surface. Same structure as the
             previous inline-edit version; here it sits below the
             form because the form is the whole page. -->
        <div
          v-if="schemaConflict"
          class="flex flex-col gap-2 p-3 rounded border border-status-warning/40 bg-status-warning/10 text-sm"
        >
          <p class="text-status-warning font-medium">
            {{
              t('admin-asset-kinds-conflict-heading', {
                count: schemaConflict.invalidCount,
              })
            }}
          </p>
          <ul class="text-xs text-secondary list-disc pl-5 flex flex-col gap-1">
            <li v-for="row in schemaConflict.sample" :key="row.id">
              <RouterLink :to="`/assets/${row.id}`" class="text-accent hover:underline">
                {{ row.name }}
              </RouterLink>
              <span class="text-tertiary"> — {{ row.error }}</span>
            </li>
          </ul>
          <p class="text-xs text-secondary">
            {{ t('admin-asset-kinds-conflict-help') }}
          </p>
        </div>

        <div class="flex justify-end gap-2 border-t border-default pt-3">
          <Button variant="secondary" type="button" @click="cancel">
            {{ t('admin-asset-kinds-cancel') }}
          </Button>
          <Button
            v-if="schemaConflict"
            variant="danger"
            type="button"
            :loading="isSaving"
            @click="submit(true)"
          >
            {{ t('admin-asset-kinds-force-save') }}
          </Button>
          <Button type="submit" :loading="isSaving">
            {{ submitLabel }}
          </Button>
        </div>
      </form>
    </div>
  </div>
</template>
