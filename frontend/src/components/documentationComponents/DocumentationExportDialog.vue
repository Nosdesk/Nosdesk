<script setup lang="ts">
/**
 * Confirms a documentation export before the download starts. The zip holds
 * the Library and Drafts only (the server leaves Archived and Trash out), so
 * the dialog says what goes in and what stays behind, with the counts the
 * sync pool already has, before an owner relies on it as a copy of
 * everything.
 *
 * The parent owns the export: closing the dialog while the zip is being
 * built doesn't cancel it, and the download still starts when it's ready.
 */
import { computed } from 'vue'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'

const props = defineProps<{
  show: boolean
  exporting: boolean
  pages: number
  collections: number
  archived: number
  trash: number
}>()

const emit = defineEmits<{ close: []; confirm: [] }>()

const leftOutCounts = computed(() => props.archived > 0 || props.trash > 0)
</script>

<template>
  <Modal :show="show" :title="$t('docs-export-title')" size="md" @close="emit('close')">
    <div class="flex flex-col gap-4 text-sm" data-testid="docs-export-dialog">
      <p class="text-secondary">{{ $t('docs-export-intro') }}</p>

      <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <div class="flex flex-col gap-1.5">
          <h3 class="text-xs uppercase tracking-wide text-tertiary font-semibold">
            {{ $t('docs-export-included-heading') }}
          </h3>
          <ul class="flex flex-col gap-1 text-secondary" data-testid="docs-export-included">
            <li class="flex items-start gap-2">
              <Icon name="check" size="xs" class="mt-1 shrink-0 text-status-success" />
              <span>{{ $t('docs-export-included-pages', { count: pages }) }}</span>
            </li>
            <li class="flex items-start gap-2">
              <Icon name="check" size="xs" class="mt-1 shrink-0 text-status-success" />
              <span>{{ $t('docs-export-included-structure', { count: collections }) }}</span>
            </li>
            <li class="flex items-start gap-2">
              <Icon name="check" size="xs" class="mt-1 shrink-0 text-status-success" />
              <span>{{ $t('docs-export-included-images') }}</span>
            </li>
          </ul>
        </div>

        <div class="flex flex-col gap-1.5">
          <h3 class="text-xs uppercase tracking-wide text-tertiary font-semibold">
            {{ $t('docs-export-excluded-heading') }}
          </h3>
          <ul class="flex flex-col gap-1 text-secondary" data-testid="docs-export-excluded">
            <template v-if="leftOutCounts">
              <li v-if="archived > 0" class="flex items-start gap-2">
                <Icon name="close" size="xs" class="mt-1 shrink-0 text-tertiary" />
                <span>{{ $t('docs-export-excluded-archived', { count: archived }) }}</span>
              </li>
              <li v-if="trash > 0" class="flex items-start gap-2">
                <Icon name="close" size="xs" class="mt-1 shrink-0 text-tertiary" />
                <span>{{ $t('docs-export-excluded-trash', { count: trash }) }}</span>
              </li>
            </template>
            <li class="flex items-start gap-2">
              <Icon name="close" size="xs" class="mt-1 shrink-0 text-tertiary" />
              <span>{{ $t('docs-export-excluded-history') }}</span>
            </li>
          </ul>
        </div>
      </div>

      <p class="text-tertiary">{{ $t('docs-export-round-trip') }}</p>

      <p v-if="exporting" class="text-tertiary" role="status" data-testid="docs-export-preparing-hint">
        {{ $t('docs-export-preparing-hint') }}
      </p>
    </div>

    <template #footer>
      <div class="modal-actions">
        <Button variant="ghost" data-testid="docs-export-dismiss" @click="emit('close')">
          {{ exporting ? $t('docs-export-close') : $t('common-cancel') }}
        </Button>
        <Button
          icon="download"
          :loading="exporting"
          data-testid="docs-export-confirm"
          @click="emit('confirm')"
        >
          {{ exporting ? $t('docs-export-preparing') : $t('docs-export-download') }}
        </Button>
      </div>
    </template>
  </Modal>
</template>
