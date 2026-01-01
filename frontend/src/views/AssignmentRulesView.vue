<script setup lang="ts">
import { ref, onMounted, computed, watch } from 'vue'
import BackButton from '@/components/common/BackButton.vue'
import AlertMessage from '@/components/common/AlertMessage.vue'
import LoadingSpinner from '@/components/common/LoadingSpinner.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import Checkbox from '@/components/common/Checkbox.vue'
import Modal from '@/components/Modal.vue'
import UserAvatar from '@/components/UserAvatar.vue'
import { assignmentRuleService } from '@/services/assignmentRuleService'
import { groupService } from '@/services/groupService'
import { categoryService } from '@/services/categoryService'
import { useDataStore } from '@/stores/dataStore'
import type {
  AssignmentRuleWithDetails,
  CreateAssignmentRuleRequest,
  UpdateAssignmentRuleRequest,
  AssignmentMethod,
  methodDisplayNames,
  methodDescriptions
} from '@/types/assignmentRule'
import type { GroupWithMemberCount } from '@/types/group'
import type { TicketCategory } from '@/types/ticket'
import type { User } from '@/types/user'

const dataStore = useDataStore()

// State
const isLoading = ref(false)
const isSaving = ref(false)
const errorMessage = ref('')
const successMessage = ref('')
const rules = ref<AssignmentRuleWithDetails[]>([])

// Modal states
const showRuleModal = ref(false)
const showDeleteConfirm = ref(false)
const editingRule = ref<AssignmentRuleWithDetails | null>(null)
const ruleToDelete = ref<AssignmentRuleWithDetails | null>(null)

// Data for selects
const groups = ref<GroupWithMemberCount[]>([])
const categories = ref<TicketCategory[]>([])
const users = ref<User[]>([])

// Form state
const ruleForm = ref<CreateAssignmentRuleRequest>({
  name: '',
  description: '',
  method: 'direct_user',
  target_user_uuid: undefined,
  target_group_id: undefined,
  trigger_on_create: true,
  trigger_on_category_change: true,
  category_id: undefined,
  is_active: true
})

// Method options
const methodOptions: { value: AssignmentMethod; label: string; description: string }[] = [
  { value: 'direct_user', label: 'Direct User', description: 'Assign directly to a specific user' },
  { value: 'group_round_robin', label: 'Round-Robin (Group)', description: 'Rotate assignment among group members evenly' },
  { value: 'group_random', label: 'Random (Group)', description: 'Randomly select a group member for each ticket' },
  { value: 'group_queue', label: 'Group Queue', description: 'Assign to group queue (users claim tickets)' }
]

// Computed
const isGroupMethod = computed(() => {
  return ['group_round_robin', 'group_random', 'group_queue'].includes(ruleForm.value.method)
})

const isDirectUserMethod = computed(() => {
  return ruleForm.value.method === 'direct_user'
})

// Load rules
const loadRules = async () => {
  isLoading.value = true
  errorMessage.value = ''

  try {
    rules.value = await assignmentRuleService.getAllRules()
  } catch (error) {
    console.error('Failed to load assignment rules:', error)
    const axiosError = error as { response?: { data?: { message?: string } } }
    errorMessage.value = axiosError.response?.data?.message || 'Failed to load assignment rules'
  } finally {
    isLoading.value = false
  }
}

// Load supporting data
const loadSupportingData = async () => {
  try {
    const [groupsData, categoriesData, usersData] = await Promise.all([
      groupService.getGroups(),
      categoryService.getCategories(),
      dataStore.getPaginatedUsers({ page: 1, pageSize: 1000 })
    ])
    groups.value = groupsData
    categories.value = categoriesData
    users.value = usersData.data
  } catch (error) {
    console.error('Failed to load supporting data:', error)
  }
}

// Open create modal
const openCreateModal = () => {
  editingRule.value = null
  ruleForm.value = {
    name: '',
    description: '',
    method: 'direct_user',
    target_user_uuid: undefined,
    target_group_id: undefined,
    trigger_on_create: true,
    trigger_on_category_change: true,
    category_id: undefined,
    is_active: true
  }
  showRuleModal.value = true
}

// Open edit modal
const openEditModal = (rule: AssignmentRuleWithDetails) => {
  editingRule.value = rule
  ruleForm.value = {
    name: rule.name,
    description: rule.description || '',
    method: rule.method,
    target_user_uuid: rule.target_user_uuid || undefined,
    target_group_id: rule.target_group_id || undefined,
    trigger_on_create: rule.trigger_on_create,
    trigger_on_category_change: rule.trigger_on_category_change,
    category_id: rule.category_id || undefined,
    is_active: rule.is_active
  }
  showRuleModal.value = true
}

// Save rule
const saveRule = async () => {
  if (!ruleForm.value.name.trim()) {
    errorMessage.value = 'Rule name is required'
    return
  }

  // Validate method requirements
  if (isDirectUserMethod.value && !ruleForm.value.target_user_uuid) {
    errorMessage.value = 'Please select a target user'
    return
  }
  if (isGroupMethod.value && !ruleForm.value.target_group_id) {
    errorMessage.value = 'Please select a target group'
    return
  }

  isSaving.value = true
  errorMessage.value = ''

  try {
    const request = {
      ...ruleForm.value,
      // Clear irrelevant fields based on method
      target_user_uuid: isDirectUserMethod.value ? ruleForm.value.target_user_uuid : undefined,
      target_group_id: isGroupMethod.value ? ruleForm.value.target_group_id : undefined
    }

    if (editingRule.value) {
      await assignmentRuleService.updateRule(editingRule.value.id, request as UpdateAssignmentRuleRequest)
      successMessage.value = 'Rule updated successfully'
    } else {
      await assignmentRuleService.createRule(request)
      successMessage.value = 'Rule created successfully'
    }

    showRuleModal.value = false
    await loadRules()
    setTimeout(() => (successMessage.value = ''), 3000)
  } catch (error) {
    const axiosError = error as { response?: { data?: string } }
    errorMessage.value = axiosError.response?.data || 'Failed to save rule'
  } finally {
    isSaving.value = false
  }
}

// Toggle rule active state
const toggleRuleActive = async (rule: AssignmentRuleWithDetails) => {
  try {
    await assignmentRuleService.updateRule(rule.id, { is_active: !rule.is_active })
    await loadRules()
  } catch (error) {
    const axiosError = error as { response?: { data?: string } }
    errorMessage.value = axiosError.response?.data || 'Failed to update rule'
  }
}

// Confirm delete
const confirmDelete = (rule: AssignmentRuleWithDetails) => {
  ruleToDelete.value = rule
  showDeleteConfirm.value = true
}

// Delete rule
const deleteRule = async () => {
  if (!ruleToDelete.value) return

  isSaving.value = true
  errorMessage.value = ''

  try {
    await assignmentRuleService.deleteRule(ruleToDelete.value.id)
    successMessage.value = 'Rule deleted successfully'
    showDeleteConfirm.value = false
    ruleToDelete.value = null
    await loadRules()
    setTimeout(() => (successMessage.value = ''), 3000)
  } catch (error) {
    const axiosError = error as { response?: { data?: string } }
    errorMessage.value = axiosError.response?.data || 'Failed to delete rule'
  } finally {
    isSaving.value = false
  }
}

// Move rule up/down in priority
const moveRule = async (rule: AssignmentRuleWithDetails, direction: 'up' | 'down') => {
  const currentIndex = rules.value.findIndex(r => r.id === rule.id)
  if (currentIndex === -1) return

  const targetIndex = direction === 'up' ? currentIndex - 1 : currentIndex + 1
  if (targetIndex < 0 || targetIndex >= rules.value.length) return

  // Swap priorities
  const currentPriority = rule.priority
  const targetPriority = rules.value[targetIndex].priority

  try {
    await assignmentRuleService.reorderRules({
      orders: [
        { id: rule.id, priority: targetPriority },
        { id: rules.value[targetIndex].id, priority: currentPriority }
      ]
    })
    await loadRules()
  } catch (error) {
    const axiosError = error as { response?: { data?: string } }
    errorMessage.value = axiosError.response?.data || 'Failed to reorder rules'
  }
}

// Get method display info
const getMethodInfo = (method: AssignmentMethod) => {
  return methodOptions.find(m => m.value === method) || { label: method, description: '' }
}

// Get target display
const getTargetDisplay = (rule: AssignmentRuleWithDetails) => {
  if (rule.method === 'direct_user' && rule.target_user) {
    return rule.target_user.name
  }
  if (rule.target_group) {
    return rule.target_group.name
  }
  return 'Not configured'
}

onMounted(() => {
  loadRules()
  loadSupportingData()
})
</script>

<template>
  <div class="flex-1">
    <!-- Navigation and actions bar -->
    <div class="pt-4 px-6 flex justify-between items-center">
      <BackButton fallbackRoute="/admin/settings" label="Back to Administration" />
      <button
        @click="openCreateModal"
        class="px-3 py-1.5 bg-accent text-white rounded-lg text-sm hover:opacity-90 font-medium transition-colors flex items-center gap-1.5"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6m0 0v6m0-6h6m-6 0H6" />
        </svg>
        New Rule
      </button>
    </div>

    <div class="flex flex-col gap-4 px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-2">
        <h1 class="text-2xl font-bold text-primary">Assignment Rules</h1>
        <p class="text-secondary mt-1">Configure automatic ticket assignment based on rules</p>
      </div>

      <!-- Info box -->
      <div class="bg-status-info/10 border border-status-info/30 rounded-lg p-4 text-sm text-status-info">
        <div class="flex items-start gap-2">
          <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
          <p>Rules are evaluated in priority order (top to bottom). The first matching rule wins. Tickets with an existing assignee are not auto-assigned.</p>
        </div>
      </div>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Loading state -->
      <LoadingSpinner v-if="isLoading" text="Loading rules..." />

      <!-- Rules list -->
      <div v-else class="flex flex-col gap-3">
        <div
          v-for="(rule, index) in rules"
          :key="rule.id"
          class="bg-surface border border-default rounded-xl hover:border-strong transition-colors"
          :class="{ 'opacity-50': !rule.is_active }"
        >
          <div class="p-4 flex items-center gap-4">
            <!-- Priority/order controls -->
            <div class="flex flex-col gap-0.5 flex-shrink-0">
              <button
                @click="moveRule(rule, 'up')"
                :disabled="index === 0"
                class="p-1 text-secondary hover:text-primary hover:bg-surface-hover rounded transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
                title="Move up (higher priority)"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M5 15l7-7 7 7" />
                </svg>
              </button>
              <span class="text-xs text-tertiary text-center w-full">{{ index + 1 }}</span>
              <button
                @click="moveRule(rule, 'down')"
                :disabled="index === rules.length - 1"
                class="p-1 text-secondary hover:text-primary hover:bg-surface-hover rounded transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
                title="Move down (lower priority)"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M19 9l-7 7-7-7" />
                </svg>
              </button>
            </div>

            <!-- Rule info -->
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 flex-wrap">
                <h3 class="font-medium text-primary">{{ rule.name }}</h3>
                <span
                  class="px-2 py-0.5 text-xs rounded-full"
                  :class="rule.is_active ? 'bg-status-success/20 text-status-success' : 'bg-surface-alt text-tertiary'"
                >
                  {{ rule.is_active ? 'Active' : 'Inactive' }}
                </span>
                <span class="px-2 py-0.5 text-xs bg-accent/20 text-accent rounded-full">
                  {{ getMethodInfo(rule.method).label }}
                </span>
              </div>
              <p v-if="rule.description" class="text-sm text-secondary mt-0.5 truncate">
                {{ rule.description }}
              </p>
              <div class="flex items-center gap-4 mt-1.5 text-xs text-tertiary">
                <span class="flex items-center gap-1">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
                  </svg>
                  {{ getTargetDisplay(rule) }}
                </span>
                <span v-if="rule.category" class="flex items-center gap-1">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M7 7h.01M7 3h5c.512 0 1.024.195 1.414.586l7 7a2 2 0 010 2.828l-7 7a2 2 0 01-2.828 0l-7-7A1.994 1.994 0 013 12V7a4 4 0 014-4z" />
                  </svg>
                  {{ rule.category.name }}
                </span>
                <span class="flex items-center gap-1">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M13 10V3L4 14h7v7l9-11h-7z" />
                  </svg>
                  <span v-if="rule.trigger_on_create && rule.trigger_on_category_change">Both triggers</span>
                  <span v-else-if="rule.trigger_on_create">On create</span>
                  <span v-else-if="rule.trigger_on_category_change">On category change</span>
                  <span v-else>No triggers</span>
                </span>
                <span v-if="rule.state" class="flex items-center gap-1">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z" />
                  </svg>
                  {{ rule.state.total_assignments }} assigned
                </span>
              </div>
            </div>

            <!-- Actions -->
            <div class="flex items-center gap-2 flex-shrink-0">
              <button
                @click="toggleRuleActive(rule)"
                class="p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
                :title="rule.is_active ? 'Deactivate rule' : 'Activate rule'"
              >
                <svg v-if="rule.is_active" xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728A9 9 0 015.636 5.636m12.728 12.728L5.636 5.636" />
                </svg>
                <svg v-else xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                </svg>
              </button>
              <button
                @click="openEditModal(rule)"
                class="p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
                title="Edit rule"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
                </svg>
              </button>
              <button
                @click="confirmDelete(rule)"
                class="p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors"
                title="Delete rule"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                </svg>
              </button>
            </div>
          </div>
        </div>

        <!-- Empty state -->
        <EmptyState
          v-if="rules.length === 0 && !isLoading"
          icon="lightning"
          title="No assignment rules yet"
          description="Create your first rule to automatically assign tickets"
          action-label="Create Rule"
          variant="card"
          @action="openCreateModal"
        />
      </div>
    </div>

    <!-- Create/Edit Rule Modal -->
    <Modal
      :show="showRuleModal"
      :title="editingRule ? 'Edit Assignment Rule' : 'Create Assignment Rule'"
      size="lg"
      @close="showRuleModal = false"
    >
      <form @submit.prevent="saveRule" class="space-y-4">
        <!-- Name -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">Rule Name</label>
          <input
            v-model="ruleForm.name"
            type="text"
            class="w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent"
            placeholder="e.g., IT Support Round-Robin"
          />
        </div>

        <!-- Description -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">Description (optional)</label>
          <textarea
            v-model="ruleForm.description"
            rows="2"
            class="w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent resize-none"
            placeholder="Describe what this rule does..."
          ></textarea>
        </div>

        <!-- Assignment Method -->
        <div>
          <label class="block text-sm font-medium text-primary mb-2">Assignment Method</label>
          <div class="grid grid-cols-2 gap-2">
            <button
              v-for="option in methodOptions"
              :key="option.value"
              type="button"
              @click="ruleForm.method = option.value"
              class="p-3 border rounded-lg text-left transition-colors"
              :class="ruleForm.method === option.value ? 'border-accent bg-accent/10 text-primary' : 'border-default bg-surface hover:bg-surface-hover text-secondary'"
            >
              <div class="font-medium text-sm">{{ option.label }}</div>
              <div class="text-xs mt-0.5 opacity-75">{{ option.description }}</div>
            </button>
          </div>
        </div>

        <!-- Target User (for direct_user method) -->
        <div v-if="isDirectUserMethod">
          <label class="block text-sm font-medium text-primary mb-1">Target User</label>
          <select
            v-model="ruleForm.target_user_uuid"
            class="w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent"
          >
            <option :value="undefined">Select a user...</option>
            <option v-for="user in users" :key="user.uuid" :value="user.uuid">
              {{ user.name }}
            </option>
          </select>
        </div>

        <!-- Target Group (for group methods) -->
        <div v-if="isGroupMethod">
          <label class="block text-sm font-medium text-primary mb-1">Target Group</label>
          <select
            v-model="ruleForm.target_group_id"
            class="w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent"
          >
            <option :value="undefined">Select a group...</option>
            <option v-for="group in groups" :key="group.id" :value="group.id">
              {{ group.name }} ({{ group.member_count }} members)
            </option>
          </select>
        </div>

        <!-- Category Filter -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">Category Filter (optional)</label>
          <select
            v-model="ruleForm.category_id"
            class="w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent"
          >
            <option :value="undefined">All categories</option>
            <option v-for="category in categories" :key="category.id" :value="category.id">
              {{ category.name }}
            </option>
          </select>
          <p class="text-xs text-tertiary mt-1">Only assign tickets with this category (leave empty for all)</p>
        </div>

        <!-- Triggers -->
        <div>
          <label class="block text-sm font-medium text-primary mb-2">Triggers</label>
          <div class="space-y-3">
            <Checkbox
              v-model="ruleForm.trigger_on_create"
              label="When a ticket is created"
            />
            <Checkbox
              v-model="ruleForm.trigger_on_category_change"
              label="When a ticket's category changes"
            />
          </div>
        </div>

        <!-- Active toggle -->
        <div class="flex items-center justify-between pt-2">
          <span class="text-sm text-secondary">Rule is active</span>
          <button
            type="button"
            @click="ruleForm.is_active = !ruleForm.is_active"
            class="relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-accent focus:ring-offset-2"
            :class="ruleForm.is_active ? 'bg-accent' : 'bg-surface-alt'"
          >
            <span
              class="pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out"
              :class="ruleForm.is_active ? 'translate-x-5' : 'translate-x-0'"
            />
          </button>
        </div>
      </form>

      <template #footer>
        <div class="flex justify-end gap-3">
          <button
            @click="showRuleModal = false"
            class="px-4 py-2 text-secondary hover:text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            @click="saveRule"
            :disabled="isSaving"
            class="px-4 py-2 bg-accent text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50"
          >
            {{ isSaving ? 'Saving...' : editingRule ? 'Update Rule' : 'Create Rule' }}
          </button>
        </div>
      </template>
    </Modal>

    <!-- Delete Confirmation Modal -->
    <Modal
      :show="showDeleteConfirm"
      title="Delete Assignment Rule"
      size="sm"
      @close="showDeleteConfirm = false"
    >
      <p class="text-secondary">
        Are you sure you want to delete the rule "{{ ruleToDelete?.name }}"? This action cannot be undone.
      </p>

      <template #footer>
        <div class="flex justify-end gap-3">
          <button
            @click="showDeleteConfirm = false"
            class="px-4 py-2 text-secondary hover:text-primary transition-colors"
          >
            Cancel
          </button>
          <button
            @click="deleteRule"
            :disabled="isSaving"
            class="px-4 py-2 bg-status-error text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50"
          >
            {{ isSaving ? 'Deleting...' : 'Delete' }}
          </button>
        </div>
      </template>
    </Modal>
  </div>
</template>
