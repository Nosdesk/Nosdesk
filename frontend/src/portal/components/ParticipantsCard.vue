<script setup lang="ts">
/**
 * People on a request. The requester can add colleagues by email (they see the
 * request in their own portal and get the team's replies) and remove them;
 * anyone who was added can leave.
 */
import { ref } from 'vue'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'
import { useRouter } from 'vue-router'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'

import { addParticipant, getMe, removeParticipant, type PortalParticipant } from '../service'

const props = defineProps<{
  ticketId: number
  participants: PortalParticipant[]
  isRequester: boolean
}>()
const emit = defineEmits<{ changed: [] }>()
const { $t: t } = useFluent()
const router = useRouter()
const me = useQuery({ key: ['portal', 'me'], query: getMe })

const email = ref('')
const busy = ref(false)
const failed = ref<string | null>(null)

async function add(): Promise<void> {
  const address = email.value.trim()
  if (!address) return
  busy.value = true
  failed.value = null
  try {
    await addParticipant(props.ticketId, address)
    email.value = ''
    emit('changed')
  } catch {
    failed.value = t('portal-people-add-failed')
  } finally {
    busy.value = false
  }
}

async function remove(person: PortalParticipant): Promise<void> {
  busy.value = true
  failed.value = null
  try {
    await removeParticipant(props.ticketId, person.uuid)
    // Leaving a request you were added to: it's no longer yours to see.
    if (person.uuid === me.data.value?.uuid) {
      void router.push('/tickets')
      return
    }
    emit('changed')
  } catch {
    failed.value = t('portal-people-remove-failed')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <section class="flex flex-col gap-3 bg-surface border border-default rounded-xl p-4">
    <h2 class="text-sm font-medium text-primary">{{ t('portal-people-title') }}</h2>
    <ul class="flex flex-col gap-2">
      <li v-for="person in participants" :key="person.uuid" class="flex items-center gap-3 text-sm">
        <span class="flex flex-col min-w-0 flex-1">
          <span class="text-primary truncate">
            {{ person.name }}
            <span v-if="person.is_requester" class="text-tertiary">· {{ t('portal-people-requester') }}</span>
          </span>
          <span v-if="person.email" class="text-xs text-secondary truncate">{{ person.email }}</span>
        </span>
        <Button
          v-if="!person.is_requester && (isRequester || person.uuid === me.data.value?.uuid)"
          variant="ghost"
          size="sm"
          :disabled="busy"
          @click="remove(person)"
        >
          {{ person.uuid === me.data.value?.uuid ? t('portal-people-leave') : t('portal-people-remove') }}
        </Button>
      </li>
    </ul>

    <form v-if="isRequester" class="flex flex-col gap-2" @submit.prevent="add">
      <div class="flex items-end gap-2">
        <FormInput
          v-model="email"
          type="email"
          class="flex-1"
          :label="t('portal-people-add-label')"
          :placeholder="t('portal-people-add-placeholder')"
          :disabled="busy"
        />
        <Button type="submit" variant="secondary" :loading="busy" :disabled="!email.trim()">
          {{ t('portal-people-add') }}
        </Button>
      </div>
      <p class="text-xs text-secondary">{{ t('portal-people-hint') }}</p>
    </form>
    <p v-if="failed" role="alert" class="text-sm text-status-error">{{ failed }}</p>
  </section>
</template>
