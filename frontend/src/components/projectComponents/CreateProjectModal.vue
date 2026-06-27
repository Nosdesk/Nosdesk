<script setup lang="ts">
/**
 * Create-project modal. Name (required) + optional description.
 * On success the caller receives the REST response and should seed
 * the sync pool immediately (`ingestCreated`) — the SSE
 * `project.created` event can arrive later.
 */
import { ref, computed } from 'vue'
import { useFluent } from 'fluent-vue'
import Modal from '@/components/Modal.vue'
import FormInput from '@/components/common/FormInput.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'
import Button from '@/components/common/Button.vue'
import { projectService } from '@nosdesk/core/services/projectService'
import { logger } from '@nosdesk/core/utils/logger'
import type { Project } from '@nosdesk/core/types/project'

const emit = defineEmits<{ close: []; created: [project: Project] }>()

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const name = ref('')
const description = ref('')
const submitting = ref(false)
const error = ref<string | null>(null)

const canSubmit = computed(() => name.value.trim().length > 0 && !submitting.value)

async function submit(): Promise<void> {
  if (!canSubmit.value) return
  submitting.value = true
  error.value = null
  try {
    const project = await projectService.createProject({
      name: name.value.trim(),
      description: description.value.trim() || null,
      status: 'active',
    })
    emit('created', project)
  } catch (e) {
    logger.error('Failed to create project', e)
    error.value = t('projects-create-error')
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <Modal :show="true" :title="t('projects-create-title')" size="sm" @close="emit('close')">
    <form id="create-project-form" class="flex flex-col gap-4" @submit.prevent="submit">
      <FormInput
        v-model="name"
        :label="t('projects-create-name-label')"
        :placeholder="t('projects-create-name-placeholder')"
        :error="error ?? undefined"
        required
        autofocus
      />
      <FormTextarea
        v-model="description"
        :label="t('projects-create-description-label')"
        :placeholder="t('projects-create-description-placeholder')"
        :rows="3"
      />
    </form>
    <template #footer>
      <div class="flex justify-end gap-2">
        <Button variant="secondary" type="button" @click="emit('close')">
          {{ t('projects-create-cancel') }}
        </Button>
        <Button
          variant="primary"
          type="button"
          :loading="submitting"
          :disabled="!canSubmit"
          @click="submit"
        >
          {{ t('projects-create-submit') }}
        </Button>
      </div>
    </template>
  </Modal>
</template>
