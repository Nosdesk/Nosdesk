<script setup lang="ts">
/**
 * Editor for an asset record type, at /new and /:id. The type's own fields
 * use the asset kinds' field builder and save their order as `field_order`,
 * since the server's jsonb sorts the schema's keys. Outcomes each carry a
 * category; the shared fields are hidden, optional or required per type.
 *
 * A built-in type nobody renamed has no label and shows its catalogue name,
 * so it reads in each person's language; an empty name field keeps it that
 * way, and a renamed built-in can go back to it.
 *
 * A save sends only what changed: a field or outcome change runs the
 * server's check of existing records, which refuses with how many would no
 * longer fit until the admin saves anyway.
 */
import { computed, ref, watch } from 'vue';
import { RouterLink, useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';

import AlertMessage from '@/components/common/AlertMessage.vue';
import BackButton from '@/components/common/BackButton.vue';
import Button from '@/components/common/Button.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import ColorHueSlider from '@/components/common/ColorHueSlider.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import Icon from '@/components/common/Icon.vue';
import SegmentedControl from '@/components/common/SegmentedControl.vue';
import type { IconName } from '@/components/common/icons';
import AttributeEditor from '@/components/assetKindComponents/AttributeEditor.vue';
import { fieldOrderOf, inFieldOrder } from '@/components/assetKindComponents/attributeSchema';
import RecordOutcomesEditor from '@/components/admin/assetRecordTypes/RecordOutcomesEditor.vue';
import {
  CORE_FIELDS,
  CORE_FIELD_MODES,
  RECORD_TYPE_ICONS,
  canonicalJson,
  compactCoreFields,
  iconLabel,
  keyFrom,
  outcomeRows,
  outcomesFromRows,
  recordTypeName,
  recordsConflict,
  sameOutcomes,
  type OutcomeRow,
  type RecordsConflict,
} from '@/components/admin/assetRecordTypes/recordTypeForm';
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery';
import { useWorkspaceQuery, useWorkspaceQueryCache } from '@/queries/workspaceQuery';
import { errorBody, errorCode, extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@nosdesk/core/stores/toast';
import { assetRecordKeys, assetRecordService } from '@nosdesk/core/services/assetRecordService';
import {
  assetRecordTypeKeys,
  assetRecordTypeService,
  type UpdateAssetRecordTypeBody,
} from '@nosdesk/core/services/assetRecordTypeService';
import type {
  AdminAssetRecordType,
  AssetRecordCoreField,
  AssetRecordCoreFieldMode,
} from '@nosdesk/core/types/asset';

const { $t: t } = useFluent();
const route = useRoute();
const router = useRouter();
const toast = useToastStore();
const queryCache = useWorkspaceQueryCache();

const routeId = computed(() => String(route.params.id ?? ''));
const isCreating = computed(() => routeId.value === 'new' || routeId.value === '');
const editingId = computed<number | null>(() => {
  if (isCreating.value) return null;
  const n = Number(routeId.value);
  return Number.isFinite(n) ? n : null;
});

const typeQuery = useWorkspaceQuery({
  key: () => [...assetRecordTypeKeys.one(editingId.value ?? 0)],
  query: () => assetRecordTypeService.get(editingId.value as number),
  enabled: () => editingId.value !== null,
  // Opened from the list, the row is already in its cache.
  placeholderData: () =>
    queryCache
      .getQueryData<AdminAssetRecordType[]>([...assetRecordTypeKeys.root])
      ?.find((row) => row.id === editingId.value),
});
const existing = computed<AdminAssetRecordType | null>(() =>
  editingId.value === null ? null : (typeQuery.data.value ?? null),
);
const isBuiltin = computed(() => existing.value?.is_builtin ?? false);

// The catalogue names a built-in's own outcomes.
const catalogueQuery = useWorkspaceQuery({
  key: [...assetRecordKeys.catalogue],
  query: () => assetRecordService.catalogue(),
  enabled: () => isBuiltin.value,
});
const catalogueEntry = computed(() =>
  existing.value?.is_builtin
    ? catalogueQuery.data.value?.find((e) => e.slug === existing.value?.slug)
    : undefined,
);
const catalogueKeys = computed<string[]>(
  () => catalogueEntry.value?.outcomes.map((o) => o.key) ?? [],
);

/** A built-in's own fields read their names from the catalogue, in each
 *  person's language; a field the admin added has only its key. */
function fieldTitle(name: string): string | undefined {
  const entry = catalogueEntry.value;
  if (!entry?.field_order.includes(name)) return undefined;
  return t(`asset-record-field-${entry.slug}-${name}`);
}

const { kinds } = useAssetKindsQuery();

type KindsMode = 'all' | 'some';
type CoreModes = Record<AssetRecordCoreField, AssetRecordCoreFieldMode>;

interface FormState {
  slug: string;
  /** '' on a built-in reading its catalogue name. */
  label: string;
  description: string;
  icon: string | null;
  color: string | null;
  /** The field schema with its properties in display order. */
  schema: Record<string, unknown>;
  outcomes: OutcomeRow[];
  coreFields: CoreModes;
  kindsMode: KindsMode;
  kinds: string[];
}

function coreModes(stored: Partial<CoreModes>): CoreModes {
  return Object.fromEntries(
    CORE_FIELDS.map((f) => [f, stored[f] ?? 'optional']),
  ) as CoreModes;
}

function blankForm(): FormState {
  return {
    slug: '',
    label: '',
    description: '',
    icon: null,
    color: null,
    schema: { type: 'object', properties: {} },
    outcomes: [],
    coreFields: coreModes({}),
    kindsMode: 'all',
    kinds: [],
  };
}

function formFrom(row: AdminAssetRecordType): FormState {
  return {
    slug: row.slug,
    label: row.label ?? '',
    description: row.description ?? '',
    icon: row.icon,
    color: row.color,
    schema: inFieldOrder(row.field_schema, row.field_order),
    outcomes: outcomeRows(row.outcomes),
    coreFields: coreModes(row.core_fields),
    kindsMode: row.applies_to_kinds === null ? 'all' : 'some',
    kinds: row.applies_to_kinds ?? [],
  };
}

const form = ref<FormState>(blankForm());
/** The form as loaded, to send only what changed. */
let base: FormState = blankForm();
const hasHydrated = ref(false);

watch(
  existing,
  (row) => {
    if (!row || hasHydrated.value) return;
    form.value = formFrom(row);
    base = formFrom(row);
    hasHydrated.value = true;
  },
  { immediate: true },
);

// A new type's slug follows its name until someone types their own.
let lastDerivedSlug = '';
watch(
  () => form.value.label,
  (label) => {
    if (!isCreating.value) return;
    const next = keyFrom(label);
    if (form.value.slug === '' || form.value.slug === lastDerivedSlug) {
      form.value.slug = next;
      lastDerivedSlug = next;
    }
  },
);

const showForm = computed(() => isCreating.value || hasHydrated.value);
const notFound = computed(
  () => !isCreating.value && !hasHydrated.value && typeQuery.error.value !== null,
);

const savedName = computed(() => (existing.value ? recordTypeName(t, existing.value) : ''));
const pageTitle = computed(() => {
  if (isCreating.value) return t('admin-asset-record-types-create-title');
  return savedName.value || t('admin-asset-record-types-edit-title');
});
const catalogueName = computed(() =>
  existing.value?.is_builtin ? t(`asset-record-type-${existing.value.slug}`) : '',
);
const canResetName = computed(() => isBuiltin.value && form.value.label.trim() !== '');

function resetName(): void {
  form.value.label = '';
}

// Fields: the builder edits the schema; its key order is the field order.
const schemaModel = computed<Record<string, unknown>>({
  get: () => form.value.schema,
  set: (next) => {
    form.value.schema = next;
  },
});

// The JSON view: for a field shape the builder can't edit, which it keeps
// as is. The text's key order is the field order.
const showJson = ref(false);
const schemaText = ref('');

function readSchemaText(): boolean {
  try {
    const parsed: unknown = JSON.parse(schemaText.value);
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) throw new Error('not an object');
    form.value.schema = parsed as Record<string, unknown>;
    return true;
  } catch (e) {
    formError.value = t('admin-asset-record-types-error-schema-json', {
      error: e instanceof Error ? e.message : String(e),
    });
    return false;
  }
}

function toggleJson(): void {
  if (showJson.value) {
    if (!readSchemaText()) return;
    formError.value = '';
  } else {
    schemaText.value = JSON.stringify(form.value.schema, null, 2);
  }
  showJson.value = !showJson.value;
}

const coreModeOptions = computed(() =>
  CORE_FIELD_MODES.map((m) => ({ value: m, label: t(`admin-asset-record-types-core-mode-${m}`) })),
);
const kindsModeOptions = computed(() => [
  { value: 'all' as KindsMode, label: t('admin-asset-record-types-kinds-all') },
  { value: 'some' as KindsMode, label: t('admin-asset-record-types-kinds-some') },
]);

/** The workspace's kinds, plus any the type names that no longer exists. */
const kindOptions = computed(() => {
  const known = kinds.value.map((k) => ({ slug: k.slug, label: k.label }));
  const missing = form.value.kinds
    .filter((slug) => !known.some((k) => k.slug === slug))
    .map((slug) => ({ slug, label: slug }));
  return [...known, ...missing];
});

function toggleKind(slug: string, on: boolean): void {
  const set = new Set(form.value.kinds);
  if (on) set.add(slug);
  else set.delete(slug);
  // Keep the kinds list's order so a save compares stably.
  form.value.kinds = kindOptions.value.map((k) => k.slug).filter((s) => set.has(s));
}

function pickIcon(name: string): void {
  form.value.icon = form.value.icon === name ? null : name;
}

const isSaving = ref(false);
const formError = ref('');
const conflict = ref<RecordsConflict | null>(null);

// A refusal describes the change that was tried; editing again drops it.
watch(
  form,
  () => {
    conflict.value = null;
  },
  { deep: true },
);

function appliesToKinds(f: FormState): string[] | null {
  return f.kindsMode === 'all' ? null : f.kinds;
}

function validate(): string {
  const f = form.value;
  if (!isBuiltin.value && !f.label.trim()) return t('admin-asset-record-types-error-name-required');
  if (isCreating.value && !f.slug.trim()) return t('admin-asset-record-types-error-slug-required');
  // A saved outcome that had no name of its own is a catalogue one.
  const catalogueNamed = new Set([
    ...catalogueKeys.value,
    ...base.outcomes.filter((row) => !row.label.trim()).map((row) => row.key),
  ]);
  const unnamed = f.outcomes.some(
    (row) => !row.label.trim() && (row.isNew || !catalogueNamed.has(row.key)),
  );
  if (unnamed) return t('admin-asset-record-types-error-outcome-name');
  if (f.kindsMode === 'some' && f.kinds.length === 0) {
    return t('admin-asset-record-types-error-kinds');
  }
  return '';
}

/** What an edit sends: only the parts that differ from the loaded type. */
function changes(): UpdateAssetRecordTypeBody {
  const f = form.value;
  const body: UpdateAssetRecordTypeBody = {};
  const label = f.label.trim() || null;
  if (label !== (base.label.trim() || null)) body.label = label;
  const description = f.description.trim() || null;
  if (description !== (base.description.trim() || null)) body.description = description;
  if (f.icon !== base.icon) body.icon = f.icon;
  if (f.color !== base.color) body.color = f.color;

  const schemaChanged = canonicalJson(f.schema) !== canonicalJson(base.schema);
  if (schemaChanged) body.field_schema = f.schema;
  const order = fieldOrderOf(f.schema);
  if (schemaChanged || order.join() !== fieldOrderOf(base.schema).join()) {
    body.field_order = order;
  }

  const outcomes = outcomesFromRows(f.outcomes);
  if (!sameOutcomes(outcomes, outcomesFromRows(base.outcomes))) body.outcomes = outcomes;

  const core = compactCoreFields(f.coreFields);
  if (canonicalJson(core) !== canonicalJson(compactCoreFields(base.coreFields))) {
    body.core_fields = core;
  }

  const kindsNow = appliesToKinds(f);
  if (canonicalJson(kindsNow) !== canonicalJson(appliesToKinds(base))) {
    body.applies_to_kinds = kindsNow;
  }
  return body;
}

async function refresh(): Promise<void> {
  await Promise.all([
    queryCache.invalidateQueries({ key: [...assetRecordTypeKeys.root] }),
    queryCache.invalidateQueries({ key: [...assetRecordKeys.types] }),
  ]);
}

/** A written type comes back with its record count, so it replaces its row
 *  in the admin caches as is; only the record form's picker refetches. */
async function store(row: AdminAssetRecordType): Promise<void> {
  queryCache.setQueryData([...assetRecordTypeKeys.one(row.id)], row);
  const list = queryCache.getQueryData<AdminAssetRecordType[]>([...assetRecordTypeKeys.root]);
  if (list) {
    queryCache.setQueryData(
      [...assetRecordTypeKeys.root],
      list.map((r) => (r.id === row.id ? row : r)),
    );
  }
  await queryCache.invalidateQueries({ key: [...assetRecordKeys.types] });
}

function refusal(error: unknown): string {
  switch (errorCode(error)) {
    case 'SLUG_TAKEN':
      return t('admin-asset-record-types-error-slug-taken');
    case 'SLUG_RESERVED':
      return t('admin-asset-record-types-error-slug-reserved');
    default:
      return extractErrorMessage(error, t('admin-asset-record-types-error-save'));
  }
}

async function submit(force = false): Promise<void> {
  if (showJson.value && !readSchemaText()) return;
  const invalid = validate();
  if (invalid) {
    formError.value = invalid;
    return;
  }
  formError.value = '';
  isSaving.value = true;
  try {
    const f = form.value;
    if (isCreating.value) {
      await assetRecordTypeService.create({
        slug: f.slug.trim(),
        label: f.label.trim(),
        description: f.description.trim() || null,
        icon: f.icon,
        color: f.color,
        field_schema: f.schema,
        field_order: fieldOrderOf(f.schema),
        core_fields: compactCoreFields(f.coreFields),
        outcomes: outcomesFromRows(f.outcomes),
        applies_to_kinds: appliesToKinds(f),
      });
      toast.success(t('admin-asset-record-types-created'));
      await refresh();
    } else if (editingId.value !== null) {
      const body = changes();
      if (Object.keys(body).length > 0) {
        await store(await assetRecordTypeService.update(editingId.value, body, force));
      }
      toast.success(t('admin-asset-record-types-saved'));
    }
    router.push({ name: 'admin-asset-record-types' });
  } catch (error) {
    const refused = recordsConflict(errorBody(error));
    if (refused) {
      conflict.value = refused;
      return;
    }
    formError.value = refusal(error);
  } finally {
    isSaving.value = false;
  }
}

// Archive, restore and delete act at once on the saved type.
const isArchiving = ref(false);
const confirmingDelete = ref(false);
const isDeleting = ref(false);
const canDelete = computed(
  () => existing.value !== null && !existing.value.is_builtin && existing.value.record_count === 0,
);

async function setArchived(archived: boolean): Promise<void> {
  if (editingId.value === null) return;
  isArchiving.value = true;
  try {
    const row = archived
      ? await assetRecordTypeService.archive(editingId.value)
      : await assetRecordTypeService.restore(editingId.value);
    await store(row);
    toast.success(
      t(archived ? 'admin-asset-record-types-archived' : 'admin-asset-record-types-restored', {
        name: savedName.value,
      }),
    );
  } catch (error) {
    formError.value = extractErrorMessage(error, t('admin-asset-record-types-error-save'));
  } finally {
    isArchiving.value = false;
  }
}

async function confirmDelete(): Promise<void> {
  if (editingId.value === null) return;
  isDeleting.value = true;
  try {
    await assetRecordTypeService.delete(editingId.value);
    toast.success(t('admin-asset-record-types-deleted', { name: savedName.value }));
    confirmingDelete.value = false;
    await refresh();
    router.push({ name: 'admin-asset-record-types' });
  } catch (error) {
    confirmingDelete.value = false;
    formError.value = extractErrorMessage(error, t('admin-asset-record-types-error-delete'));
  } finally {
    isDeleting.value = false;
  }
}

function cancel(): void {
  router.push({ name: 'admin-asset-record-types' });
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-5xl">
      <div class="flex flex-col gap-1">
        <BackButton
          :fallback-route="'/admin/asset-record-types'"
          :label="t('admin-asset-record-types-back-to-list')"
          compact
        />
        <div class="flex flex-wrap items-center gap-2">
          <h1 class="text-xl sm:text-2xl font-bold text-primary" data-test="page-title">
            {{ pageTitle }}
          </h1>
          <span
            v-if="existing?.archived_at"
            class="text-xs px-1.5 py-0.5 rounded bg-surface-alt text-tertiary"
          >
            {{ t('admin-asset-record-types-archived-tag') }}
          </span>
        </div>
        <p v-if="existing" class="text-sm text-secondary">
          {{ t('admin-asset-record-types-record-count', { count: existing.record_count }) }}
        </p>
      </div>

      <AlertMessage v-if="formError" type="error" :message="formError" />
      <AlertMessage
        v-if="notFound"
        type="error"
        :message="t('admin-asset-record-types-not-found')"
      />

      <form v-if="showForm" class="flex flex-col gap-6" @submit.prevent="submit()">
        <section class="flex flex-col gap-3">
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div class="flex flex-col gap-1">
              <FormInput
                v-model="form.label"
                :label="t('admin-asset-record-types-field-name')"
                :placeholder="catalogueName"
                :description="isBuiltin ? t('admin-asset-record-types-field-name-builtin-hint') : undefined"
                :required="!isBuiltin"
                maxlength="255"
                data-test="name-input"
              />
              <Button
                v-if="canResetName"
                variant="ghost"
                size="sm"
                icon="reset"
                class="self-start"
                @click="resetName"
              >
                {{ t('admin-asset-record-types-reset-name', { name: catalogueName }) }}
              </Button>
            </div>
            <FormInput
              v-model="form.slug"
              :label="t('admin-asset-record-types-field-slug')"
              :disabled="!isCreating"
              :description="
                isCreating
                  ? t('admin-asset-record-types-field-slug-hint')
                  : t('admin-asset-record-types-field-slug-locked')
              "
              :required="isCreating"
              maxlength="64"
            />
          </div>
          <FormInput
            v-model="form.description"
            :label="t('admin-asset-record-types-field-description')"
            data-test="description-input"
          />
        </section>

        <section class="flex flex-col gap-3" :aria-label="t('admin-asset-record-types-look-heading')">
          <h2 class="text-sm font-medium text-primary">
            {{ t('admin-asset-record-types-look-heading') }}
          </h2>
          <div class="flex flex-col gap-1.5">
            <span class="text-xs font-medium text-tertiary uppercase tracking-wide">
              {{ t('admin-asset-record-types-field-icon') }}
            </span>
            <div class="flex flex-wrap gap-1">
              <button
                v-for="name in RECORD_TYPE_ICONS"
                :key="name"
                type="button"
                :title="iconLabel(name)"
                :aria-label="iconLabel(name)"
                :aria-pressed="form.icon === name"
                :class="[
                  'flex items-center justify-center w-9 h-9 rounded-md border transition-colors',
                  form.icon === name
                    ? 'border-accent bg-accent/10 text-accent'
                    : 'border-default text-secondary hover:text-primary hover:bg-surface-hover',
                ]"
                @click="pickIcon(name)"
              >
                <Icon :name="name as IconName" />
              </button>
            </div>
          </div>
          <div class="flex flex-col gap-1.5">
            <span class="text-xs font-medium text-tertiary uppercase tracking-wide">
              {{ t('admin-asset-record-types-field-color') }}
            </span>
            <div v-if="form.color" class="flex flex-col sm:flex-row sm:items-center gap-2">
              <ColorHueSlider v-model="form.color" class="flex-1" />
              <Button variant="ghost" size="sm" class="self-start" @click="form.color = null">
                {{ t('admin-asset-record-types-color-remove') }}
              </Button>
            </div>
            <Button
              v-else
              variant="secondary"
              size="sm"
              icon="paint"
              class="self-start"
              @click="form.color = '#6366f1'"
            >
              {{ t('admin-asset-record-types-color-add') }}
            </Button>
          </div>
        </section>

        <section class="flex flex-col gap-3" :aria-label="t('admin-asset-record-types-fields-heading')">
          <div class="flex flex-wrap items-start gap-2">
            <div class="flex-1 min-w-0 flex flex-col gap-1">
              <h2 class="text-sm font-medium text-primary">
                {{ t('admin-asset-record-types-fields-heading') }}
              </h2>
              <p class="text-xs text-tertiary">{{ t('admin-asset-record-types-fields-hint') }}</p>
            </div>
            <Button
              variant="ghost"
              size="sm"
              :icon="showJson ? 'documentEdit' : 'document'"
              @click="toggleJson"
            >
              {{ showJson ? t('admin-asset-kinds-view-builder') : t('admin-asset-kinds-view-json') }}
            </Button>
          </div>
          <FormTextarea
            v-if="showJson"
            v-model="schemaText"
            :rows="12"
            class="font-mono"
            :aria-label="t('admin-asset-record-types-fields-heading')"
          />
          <AttributeEditor
            v-else
            v-model="schemaModel"
            :field-title="fieldTitle"
            :empty-title="t('admin-asset-record-types-fields-empty-title')"
            :empty-description="t('admin-asset-record-types-fields-empty-description')"
            :add-label="t('admin-asset-record-types-fields-add')"
          />
        </section>

        <section class="flex flex-col gap-3" :aria-label="t('admin-asset-record-types-outcomes-heading')">
          <div class="flex flex-col gap-1">
            <h2 class="text-sm font-medium text-primary">
              {{ t('admin-asset-record-types-outcomes-heading') }}
            </h2>
            <p class="text-xs text-tertiary">{{ t('admin-asset-record-types-outcomes-hint') }}</p>
          </div>
          <RecordOutcomesEditor
            v-model="form.outcomes"
            :type-slug="form.slug"
            :catalogue-keys="catalogueKeys"
          />
        </section>

        <section class="flex flex-col gap-3" :aria-label="t('admin-asset-record-types-core-heading')">
          <div class="flex flex-col gap-1">
            <h2 class="text-sm font-medium text-primary">
              {{ t('admin-asset-record-types-core-heading') }}
            </h2>
            <p class="text-xs text-tertiary">{{ t('admin-asset-record-types-core-hint') }}</p>
          </div>
          <ul class="flex flex-col bg-surface border border-default rounded-lg overflow-hidden">
            <li
              v-for="(field, i) in CORE_FIELDS"
              :key="field"
              :class="[
                'flex flex-col sm:flex-row sm:items-center gap-2 p-3',
                i < CORE_FIELDS.length - 1 ? 'border-b border-default' : '',
              ]"
            >
              <span class="flex-1 min-w-0 flex flex-col gap-0.5">
                <span class="text-sm text-primary">{{ t(`admin-asset-record-types-core-${field}`) }}</span>
                <span class="text-xs text-tertiary">{{ t(`admin-asset-record-types-core-${field}-hint`) }}</span>
              </span>
              <SegmentedControl
                v-model="form.coreFields[field]"
                :options="coreModeOptions"
                :aria-label="t(`admin-asset-record-types-core-${field}`)"
                size="sm"
              />
            </li>
          </ul>
        </section>

        <section class="flex flex-col gap-3" :aria-label="t('admin-asset-record-types-kinds-heading')">
          <div class="flex flex-col gap-1">
            <h2 class="text-sm font-medium text-primary">
              {{ t('admin-asset-record-types-kinds-heading') }}
            </h2>
            <p class="text-xs text-tertiary">{{ t('admin-asset-record-types-kinds-hint') }}</p>
          </div>
          <SegmentedControl
            v-model="form.kindsMode"
            :options="kindsModeOptions"
            :aria-label="t('admin-asset-record-types-kinds-heading')"
            size="sm"
            class="self-start"
          />
          <div v-if="form.kindsMode === 'some'" class="flex flex-wrap gap-x-4 gap-y-2">
            <Checkbox
              v-for="kind in kindOptions"
              :key="kind.slug"
              :model-value="form.kinds.includes(kind.slug)"
              :label="kind.label"
              size="sm"
              @update:model-value="(on: boolean) => toggleKind(kind.slug, on)"
            />
          </div>
        </section>

        <div
          v-if="conflict"
          data-test="records-conflict"
          class="flex flex-col gap-2 p-3 rounded-lg border border-status-warning/40 bg-status-warning/10 text-sm"
        >
          <p class="text-status-warning font-medium">
            {{ t('admin-asset-record-types-conflict-heading', { count: conflict.invalidCount }) }}
          </p>
          <ul v-if="conflict.sample.length > 0" class="text-xs text-secondary list-disc pl-5 flex flex-col gap-1">
            <li v-for="row in conflict.sample" :key="row.id">
              <RouterLink
                :to="{ path: `/assets/${row.asset_id}`, query: { record: String(row.id) } }"
                class="text-accent hover:underline"
              >
                {{
                  row.asset_tag
                    ? t('admin-asset-record-types-conflict-asset-tagged', {
                        name: row.asset_name,
                        tag: row.asset_tag,
                      })
                    : row.asset_name
                }}
              </RouterLink>
              <span class="text-tertiary">: {{ row.error }}</span>
            </li>
          </ul>
          <p class="text-xs text-secondary">{{ t('admin-asset-record-types-conflict-help') }}</p>
        </div>

        <div class="flex flex-wrap items-center gap-2 border-t border-default pt-3">
          <template v-if="existing">
            <Button
              v-if="existing.archived_at"
              variant="secondary"
              icon="restore"
              :loading="isArchiving"
              @click="setArchived(false)"
            >
              {{ t('admin-asset-record-types-restore') }}
            </Button>
            <Button
              v-else
              variant="secondary"
              icon="archive"
              :loading="isArchiving"
              @click="setArchived(true)"
            >
              {{ t('admin-asset-record-types-archive') }}
            </Button>
            <Button
              v-if="canDelete"
              variant="ghost-danger"
              icon="trash"
              @click="confirmingDelete = true"
            >
              {{ t('admin-asset-record-types-delete') }}
            </Button>
          </template>
          <div class="flex items-center gap-2 ml-auto">
            <Button variant="secondary" @click="cancel">
              {{ t('admin-asset-record-types-cancel') }}
            </Button>
            <Button
              v-if="conflict"
              variant="danger"
              :loading="isSaving"
              data-test="force-save"
              @click="submit(true)"
            >
              {{ t('admin-asset-record-types-force-save') }}
            </Button>
            <Button v-else type="submit" :loading="isSaving" data-test="save">
              {{ isCreating ? t('admin-asset-record-types-create') : t('admin-asset-record-types-save') }}
            </Button>
          </div>
        </div>
      </form>
    </div>

    <ConfirmModal
      :show="confirmingDelete"
      :title="t('admin-asset-record-types-delete-title')"
      :message="t('admin-asset-record-types-delete-confirm', { name: savedName })"
      :confirm-label="t('admin-asset-record-types-delete')"
      :loading="isDeleting"
      variant="danger"
      @confirm="confirmDelete"
      @close="confirmingDelete = false"
    />
  </div>
</template>
