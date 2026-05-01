import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { logger } from '@/utils/logger'
import { workflowStatesService } from '@/services/workflowStatesService'
import type { WorkflowState, WorkflowStateCategory } from '@/types/workflow'

/**
 * Workflow states are a small, slow-moving set — typically 6 to ~20
 * rows — read on most ticket-touching screens. The store loads them
 * once after auth and keeps them in memory until the user signs out
 * or an admin write triggers a `workflow_states_changed` SSE event
 * (wired in a later commit).
 */
export const useWorkflowStatesStore = defineStore('workflowStates', () => {
  const states = ref<WorkflowState[]>([])
  const loaded = ref(false)
  const loading = ref(false)
  const error = ref<string | null>(null)

  let inflight: Promise<WorkflowState[]> | null = null

  async function load(force = false): Promise<WorkflowState[]> {
    if (loaded.value && !force) return states.value
    if (inflight) return inflight

    loading.value = true
    error.value = null

    inflight = (async () => {
      try {
        const next = await workflowStatesService.list()
        states.value = next
        loaded.value = true
        return next
      } catch (e) {
        logger.error('Failed to load workflow states', e)
        error.value = e instanceof Error ? e.message : 'Failed to load workflow states'
        return states.value
      } finally {
        loading.value = false
        inflight = null
      }
    })()

    return inflight
  }

  function reset() {
    states.value = []
    loaded.value = false
    error.value = null
  }

  function findById(id: number): WorkflowState | undefined {
    return states.value.find((s) => s.id === id)
  }

  /** Default state for new tickets (the workspace-default row). */
  const defaultState = computed<WorkflowState | undefined>(() =>
    states.value.find((s) => s.is_default && !s.archived_at),
  )

  /** Active states grouped by category, each group ordered by position. */
  const byCategory = computed<Record<WorkflowStateCategory, WorkflowState[]>>(() => {
    const out: Record<WorkflowStateCategory, WorkflowState[]> = {
      triage: [],
      backlog: [],
      active: [],
      in_review: [],
      done: [],
      cancelled: [],
    }
    for (const s of states.value) {
      if (s.archived_at) continue
      out[s.category].push(s)
    }
    for (const cat of Object.keys(out) as WorkflowStateCategory[]) {
      out[cat].sort((a, b) => a.position - b.position)
    }
    return out
  })

  return {
    states,
    loaded,
    loading,
    error,
    defaultState,
    byCategory,
    load,
    reset,
    findById,
  }
})
