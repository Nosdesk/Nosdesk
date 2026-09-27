<script setup lang="ts">
/**
 * "Is it fixed?" for the requester. On an open request it offers to close it;
 * on a closed one it asks, with "no" handing over to the reply box. An answer
 * arriving from the resolved email (`answer`) is recorded here, by the page,
 * so a mail scanner fetching the link changes nothing.
 */
import { computed, onMounted, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'

import { isClosed, resolveMyTicket, type PortalRating, type PortalTicket } from '../service'

const props = defineProps<{
  ticket: PortalTicket
  rating: PortalRating | null
  /** "fixed" or "not_fixed" from the email link, once. */
  answer?: string | null
}>()
const emit = defineEmits<{ changed: []; stillNeedsHelp: [] }>()
const { $t: t } = useFluent()

const busy = ref(false)
const failed = ref(false)
const justResolved = ref(false)
const note = ref('')
const noteSent = ref(false)

const closed = computed(() => isClosed(props.ticket))
const saidFixed = computed(() => props.rating?.rating === 'good')

async function resolve(comment?: string): Promise<void> {
  busy.value = true
  failed.value = false
  try {
    await resolveMyTicket(props.ticket.id, comment)
    if (comment) noteSent.value = true
    else justResolved.value = true
    emit('changed')
  } catch {
    failed.value = true
  } finally {
    busy.value = false
  }
}

onMounted(() => {
  if (props.answer === 'fixed' && !saidFixed.value) void resolve()
  if (props.answer === 'not_fixed') emit('stillNeedsHelp')
})
</script>

<template>
  <section class="flex flex-col gap-3 bg-surface border border-default rounded-xl p-4">
    <template v-if="justResolved">
      <p class="text-sm text-primary">{{ t('portal-resolve-thanks') }}</p>
      <p v-if="noteSent" class="text-sm text-secondary">{{ t('portal-resolve-noted') }}</p>
      <form v-else class="flex flex-col gap-2" @submit.prevent="resolve(note.trim())">
        <FormTextarea
          v-model="note"
          :label="t('portal-resolve-note-label')"
          :rows="2"
          resize="vertical"
          :disabled="busy"
        />
        <Button type="submit" variant="secondary" class="self-end" :loading="busy" :disabled="!note.trim()">
          {{ t('portal-resolve-note-send') }}
        </Button>
      </form>
    </template>

    <p v-else-if="saidFixed" class="text-sm text-secondary">{{ t('portal-resolve-rated-good') }}</p>

    <template v-else-if="closed">
      <p class="text-sm font-medium text-primary">{{ t('portal-resolve-question') }}</p>
      <div class="flex flex-wrap gap-2">
        <Button variant="secondary" :loading="busy" @click="resolve()">{{ t('portal-resolve-yes') }}</Button>
        <Button variant="secondary" :disabled="busy" @click="emit('stillNeedsHelp')">{{ t('portal-resolve-no') }}</Button>
      </div>
    </template>

    <template v-else>
      <p class="text-sm text-secondary">{{ t('portal-resolve-hint-open') }}</p>
      <Button variant="secondary" class="self-start" :loading="busy" @click="resolve()">
        {{ t('portal-resolve-yes') }}
      </Button>
    </template>

    <p v-if="failed" role="alert" class="text-sm text-status-error">{{ t('portal-resolve-failed') }}</p>
  </section>
</template>
