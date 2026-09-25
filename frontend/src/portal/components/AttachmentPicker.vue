<script setup lang="ts">
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'

import { uploadFile, type PortalAttachment } from '../service'

const MAX_FILES = 5

// Staged uploads; the parent sends their ids with the reply or request.
const files = defineModel<PortalAttachment[]>({ required: true })
const props = defineProps<{ disabled?: boolean }>()

const { $t: t } = useFluent()
const input = ref<HTMLInputElement | null>(null)
const uploading = ref(0)
const failed = ref<string[]>([])

const full = computed(() => files.value.length + uploading.value >= MAX_FILES)

async function onPick(event: Event): Promise<void> {
  const picked = Array.from((event.target as HTMLInputElement).files ?? [])
  ;(event.target as HTMLInputElement).value = ''
  failed.value = []
  const room = MAX_FILES - files.value.length - uploading.value
  await Promise.all(
    picked.slice(0, Math.max(room, 0)).map(async (file) => {
      uploading.value++
      try {
        files.value = [...files.value, await uploadFile(file)]
      } catch {
        failed.value = [...failed.value, file.name]
      } finally {
        uploading.value--
      }
    }),
  )
}

function remove(id: number): void {
  files.value = files.value.filter((f) => f.id !== id)
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <div class="flex items-center gap-3 flex-wrap">
      <Button
        variant="secondary"
        size="sm"
        icon="paperclip"
        :loading="uploading > 0"
        :disabled="props.disabled || full"
        @click="input?.click()"
      >
        {{ t('portal-attach-files') }}
      </Button>
      <span class="text-xs text-tertiary">{{ t('portal-attach-hint') }}</span>
      <input ref="input" type="file" multiple class="hidden" @change="onPick" />
    </div>
    <ul v-if="files.length" class="flex flex-wrap gap-2">
      <li
        v-for="file in files"
        :key="file.id"
        class="inline-flex items-center gap-1.5 text-xs pl-2 pr-1 py-1 rounded-md bg-surface-alt border border-default text-secondary"
      >
        <Icon name="paperclip" />
        <span class="truncate max-w-[14rem]">{{ file.name }}</span>
        <button
          type="button"
          class="p-0.5 rounded hover:bg-surface-hover hover:text-primary"
          :aria-label="t('portal-attach-remove', { name: file.name })"
          :disabled="props.disabled"
          @click="remove(file.id)"
        >
          <Icon name="close" />
        </button>
      </li>
    </ul>
    <p v-for="name in failed" :key="name" role="alert" class="text-xs text-status-error">
      {{ t('portal-attach-failed', { name }) }}
    </p>
  </div>
</template>
