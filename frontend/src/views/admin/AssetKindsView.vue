<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import LoadingSpinner from '@/components/common/LoadingSpinner.vue';
import Icon from '@/components/common/Icon.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import {
  assetKindsService,
  ASSET_KIND_CATEGORIES,
  type AssetKind,
  type AssetKindCategory,
  type CreateAssetKindBody,
} from '@/services/assetKindsService';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const kinds = ref<AssetKind[]>([]);
const isLoading = ref(false);
const errorMessage = ref('');
const successMessage = ref('');

const editingId = ref<number | null>(null);
const editDraft = ref<{
  label: string;
  description: string;
  icon: string;
  sort_order: number;
  category: AssetKindCategory;
  attribute_schema: string;
} | null>(null);

const newKind = ref<{
  slug: string;
  label: string;
  description: string;
  icon: string;
  sort_order: number;
  category: AssetKindCategory;
  attribute_schema: string;
}>({
  slug: '',
  label: '',
  description: '',
  icon: '',
  sort_order: 100,
  category: 'generic',
  attribute_schema: '{\n  "type": "object",\n  "properties": {}\n}',
});

const pendingDelete = ref<AssetKind | null>(null);

const builtinKinds = computed(() => kinds.value.filter((k) => k.is_builtin));
const customKinds = computed(() => kinds.value.filter((k) => !k.is_builtin));

async function reload() {
  isLoading.value = true;
  errorMessage.value = '';
  try {
    kinds.value = await assetKindsService.list();
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('admin-asset-kinds-error-load');
  } finally {
    isLoading.value = false;
  }
}

function flash(message: string) {
  successMessage.value = message;
  setTimeout(() => {
    if (successMessage.value === message) successMessage.value = '';
  }, 2500);
}

/**
 * Parse the schema textarea. Empty string becomes the
 * canonical empty-properties schema so admins can leave the
 * field alone when their kind has no per-kind attributes.
 */
function parseSchema(input: string): Record<string, unknown> {
  const trimmed = input.trim();
  if (!trimmed) {
    return { type: 'object', properties: {} };
  }
  return JSON.parse(trimmed);
}

function startEdit(kind: AssetKind) {
  editingId.value = kind.id;
  editDraft.value = {
    label: kind.label,
    description: kind.description ?? '',
    icon: kind.icon ?? '',
    sort_order: kind.sort_order,
    category: kind.category,
    attribute_schema: JSON.stringify(kind.attribute_schema, null, 2),
  };
}

function cancelEdit() {
  editingId.value = null;
  editDraft.value = null;
}

async function saveEdit(kind: AssetKind) {
  if (!editDraft.value) return;
  const draft = editDraft.value;
  const label = draft.label.trim();
  if (!label) {
    errorMessage.value = t('admin-asset-kinds-error-label-required');
    return;
  }
  let schema: Record<string, unknown>;
  try {
    schema = parseSchema(draft.attribute_schema);
  } catch (e) {
    errorMessage.value = t('admin-asset-kinds-error-bad-schema-json', {
      error: e instanceof Error ? e.message : String(e),
    });
    return;
  }
  try {
    await assetKindsService.update(kind.id, {
      label,
      // null clears, empty string leaves alone; we treat blank
      // input as "no description" since the user just cleared it.
      description: draft.description.trim() === '' ? null : draft.description.trim(),
      icon: draft.icon.trim() === '' ? null : draft.icon.trim(),
      sort_order: draft.sort_order,
      category: draft.category,
      attribute_schema: schema,
    });
    await reload();
    cancelEdit();
    flash(t('admin-asset-kinds-saved'));
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('admin-asset-kinds-error-save');
  }
}

async function createKind() {
  const slug = newKind.value.slug.trim();
  const label = newKind.value.label.trim();
  if (!slug) {
    errorMessage.value = t('admin-asset-kinds-error-slug-required');
    return;
  }
  if (!label) {
    errorMessage.value = t('admin-asset-kinds-error-label-required');
    return;
  }
  let schema: Record<string, unknown>;
  try {
    schema = parseSchema(newKind.value.attribute_schema);
  } catch (e) {
    errorMessage.value = t('admin-asset-kinds-error-bad-schema-json', {
      error: e instanceof Error ? e.message : String(e),
    });
    return;
  }
  const body: CreateAssetKindBody = {
    slug,
    label,
    description: newKind.value.description.trim() || null,
    icon: newKind.value.icon.trim() || null,
    sort_order: newKind.value.sort_order,
    category: newKind.value.category,
    attribute_schema: schema,
  };
  try {
    await assetKindsService.create(body);
    await reload();
    newKind.value = {
      slug: '',
      label: '',
      description: '',
      icon: '',
      sort_order: 100,
      category: 'generic',
      attribute_schema: '{\n  "type": "object",\n  "properties": {}\n}',
    };
    flash(t('admin-asset-kinds-created'));
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('admin-asset-kinds-error-create');
  }
}

async function confirmDelete() {
  if (!pendingDelete.value) return;
  const kind = pendingDelete.value;
  try {
    await assetKindsService.delete(kind.id);
    pendingDelete.value = null;
    await reload();
    flash(t('admin-asset-kinds-deleted', { label: kind.label }));
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('admin-asset-kinds-error-delete');
  }
}

onMounted(reload);
</script>

<template>
  <div class="flex-1 flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-5xl">
    <div>
      <h1 class="text-xl lg:text-2xl font-bold text-primary">
        {{ $t('admin-asset-kinds-title') }}
      </h1>
      <p class="text-secondary text-sm lg:text-base mt-1">
        {{ $t('admin-asset-kinds-description') }}
      </p>
    </div>

    <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />
    <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

    <LoadingSpinner v-if="isLoading && kinds.length === 0" />

    <section v-else class="flex flex-col gap-6">
      <!-- Built-in kinds (read-only display, schema can be edited). -->
      <div>
        <h2 class="text-base font-semibold text-primary mb-2">
          {{ $t('admin-asset-kinds-builtin-heading') }}
        </h2>
        <p class="text-sm text-secondary mb-3">
          {{ $t('admin-asset-kinds-builtin-description') }}
        </p>
        <div class="border border-default rounded-lg divide-y divide-default bg-surface">
          <div
            v-for="kind in builtinKinds"
            :key="kind.id"
            class="p-4"
          >
            <div class="flex items-start justify-between gap-3 flex-wrap">
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="font-medium text-primary">{{ kind.label }}</span>
                  <code class="text-xs text-tertiary">{{ kind.slug }}</code>
                  <span class="text-xs text-accent">{{ $t('admin-asset-kinds-builtin-tag') }}</span>
                </div>
                <p v-if="kind.description" class="text-sm text-secondary mt-1">
                  {{ kind.description }}
                </p>
              </div>
              <button
                v-if="editingId !== kind.id"
                class="text-sm text-accent hover:underline"
                @click="startEdit(kind)"
              >
                {{ $t('admin-asset-kinds-edit-schema') }}
              </button>
            </div>
            <div v-if="editingId === kind.id && editDraft" class="mt-3 flex flex-col gap-3">
              <label class="text-xs font-medium text-secondary">
                {{ $t('admin-asset-kinds-field-label') }}
                <input
                  v-model="editDraft.label"
                  class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm"
                />
              </label>
              <label class="text-xs font-medium text-secondary">
                {{ $t('admin-asset-kinds-field-description') }}
                <input
                  v-model="editDraft.description"
                  class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm"
                />
              </label>
              <label class="text-xs font-medium text-secondary">
                {{ $t('admin-asset-kinds-field-attribute-schema') }}
                <textarea
                  v-model="editDraft.attribute_schema"
                  rows="8"
                  class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 font-mono text-xs"
                />
              </label>
              <div class="flex justify-end gap-2">
                <button
                  class="px-3 py-1 text-sm rounded border border-default text-secondary hover:text-primary"
                  @click="cancelEdit"
                >
                  {{ $t('admin-asset-kinds-cancel') }}
                </button>
                <button
                  class="px-3 py-1 text-sm rounded bg-accent text-on-accent hover:bg-accent-strong"
                  @click="saveEdit(kind)"
                >
                  {{ $t('admin-asset-kinds-save') }}
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Custom kinds (full CRUD). -->
      <div>
        <h2 class="text-base font-semibold text-primary mb-2">
          {{ $t('admin-asset-kinds-custom-heading') }}
        </h2>
        <p class="text-sm text-secondary mb-3">
          {{ $t('admin-asset-kinds-custom-description') }}
        </p>

        <div
          v-if="customKinds.length === 0"
          class="text-sm text-secondary italic mb-4"
        >
          {{ $t('admin-asset-kinds-custom-empty') }}
        </div>

        <div v-else class="border border-default rounded-lg divide-y divide-default bg-surface mb-4">
          <div
            v-for="kind in customKinds"
            :key="kind.id"
            class="p-4"
          >
            <div class="flex items-start justify-between gap-3 flex-wrap">
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="font-medium text-primary">{{ kind.label }}</span>
                  <code class="text-xs text-tertiary">{{ kind.slug }}</code>
                </div>
                <p v-if="kind.description" class="text-sm text-secondary mt-1">
                  {{ kind.description }}
                </p>
              </div>
              <div class="flex items-center gap-2">
                <button
                  v-if="editingId !== kind.id"
                  class="text-sm text-accent hover:underline"
                  @click="startEdit(kind)"
                >
                  {{ $t('admin-asset-kinds-edit') }}
                </button>
                <button
                  class="text-sm text-danger hover:underline inline-flex items-center gap-1"
                  @click="pendingDelete = kind"
                >
                  <Icon name="trash" />
                  {{ $t('admin-asset-kinds-delete') }}
                </button>
              </div>
            </div>
            <div v-if="editingId === kind.id && editDraft" class="mt-3 flex flex-col gap-3">
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <label class="text-xs font-medium text-secondary">
                  {{ $t('admin-asset-kinds-field-label') }}
                  <input
                    v-model="editDraft.label"
                    class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm"
                  />
                </label>
                <label class="text-xs font-medium text-secondary">
                  {{ $t('admin-asset-kinds-field-icon') }}
                  <input
                    v-model="editDraft.icon"
                    class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm"
                  />
                </label>
              </div>
              <label class="text-xs font-medium text-secondary">
                {{ $t('admin-asset-kinds-field-description') }}
                <input
                  v-model="editDraft.description"
                  class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm"
                />
              </label>
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <label class="text-xs font-medium text-secondary">
                  {{ $t('admin-asset-kinds-field-sort-order') }}
                  <input
                    v-model.number="editDraft.sort_order"
                    type="number"
                    class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm"
                  />
                </label>
                <label class="text-xs font-medium text-secondary">
                  {{ $t('admin-asset-kinds-field-category') }}
                  <select
                    v-model="editDraft.category"
                    class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm"
                  >
                    <option v-for="c in ASSET_KIND_CATEGORIES" :key="c" :value="c">
                      {{ $t(`admin-asset-kinds-category-${c}`) }}
                    </option>
                  </select>
                </label>
              </div>
              <label class="text-xs font-medium text-secondary">
                {{ $t('admin-asset-kinds-field-attribute-schema') }}
                <textarea
                  v-model="editDraft.attribute_schema"
                  rows="10"
                  class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 font-mono text-xs"
                />
              </label>
              <div class="flex justify-end gap-2">
                <button
                  class="px-3 py-1 text-sm rounded border border-default text-secondary hover:text-primary"
                  @click="cancelEdit"
                >
                  {{ $t('admin-asset-kinds-cancel') }}
                </button>
                <button
                  class="px-3 py-1 text-sm rounded bg-accent text-on-accent hover:bg-accent-strong"
                  @click="saveEdit(kind)"
                >
                  {{ $t('admin-asset-kinds-save') }}
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Create form -->
        <div class="border border-default rounded-lg p-4 bg-surface">
          <h3 class="text-sm font-semibold text-primary mb-3">
            {{ $t('admin-asset-kinds-create-heading') }}
          </h3>
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3 mb-3">
            <label class="text-xs font-medium text-secondary">
              {{ $t('admin-asset-kinds-field-slug') }}
              <input
                v-model="newKind.slug"
                :placeholder="$t('admin-asset-kinds-field-slug-placeholder')"
                class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm font-mono"
              />
            </label>
            <label class="text-xs font-medium text-secondary">
              {{ $t('admin-asset-kinds-field-label') }}
              <input
                v-model="newKind.label"
                class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm"
              />
            </label>
          </div>
          <div class="grid grid-cols-1 sm:grid-cols-3 gap-3 mb-3">
            <label class="text-xs font-medium text-secondary">
              {{ $t('admin-asset-kinds-field-icon') }}
              <input
                v-model="newKind.icon"
                class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm"
              />
            </label>
            <label class="text-xs font-medium text-secondary">
              {{ $t('admin-asset-kinds-field-sort-order') }}
              <input
                v-model.number="newKind.sort_order"
                type="number"
                class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm"
              />
            </label>
            <label class="text-xs font-medium text-secondary">
              {{ $t('admin-asset-kinds-field-category') }}
              <select
                v-model="newKind.category"
                class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm"
              >
                <option v-for="c in ASSET_KIND_CATEGORIES" :key="c" :value="c">
                  {{ $t(`admin-asset-kinds-category-${c}`) }}
                </option>
              </select>
            </label>
          </div>
          <label class="text-xs font-medium text-secondary block mb-3">
            {{ $t('admin-asset-kinds-field-description') }}
            <input
              v-model="newKind.description"
              class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 text-sm"
            />
          </label>
          <label class="text-xs font-medium text-secondary block mb-3">
            {{ $t('admin-asset-kinds-field-attribute-schema') }}
            <textarea
              v-model="newKind.attribute_schema"
              rows="10"
              class="mt-1 w-full bg-surface-alt text-primary rounded border border-default px-2 py-1 font-mono text-xs"
            />
          </label>
          <div class="flex justify-end">
            <button
              class="px-3 py-1 text-sm rounded bg-accent text-on-accent hover:bg-accent-strong"
              @click="createKind"
            >
              {{ $t('admin-asset-kinds-create-button') }}
            </button>
          </div>
        </div>
      </div>
    </section>

    <ConfirmModal
      :show="pendingDelete !== null"
      :title="$t('admin-asset-kinds-delete-confirm-title')"
      :confirm-label="$t('admin-asset-kinds-delete')"
      variant="danger"
      :message="pendingDelete ? $t('admin-asset-kinds-delete-confirm', { label: pendingDelete.label }) : ''"
      @confirm="confirmDelete"
      @close="pendingDelete = null"
    />
  </div>
</template>
