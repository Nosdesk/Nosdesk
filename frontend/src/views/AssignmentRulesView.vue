<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery, useQueryCache } from '@pinia/colada'

import AlertMessage from '@/components/common/AlertMessage.vue'
import Skeleton from '@/components/common/Skeleton.vue'
import SkeletonBar from '@/components/common/SkeletonBar.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import Icon from '@/components/common/Icon.vue'
import Checkbox from '@/components/common/Checkbox.vue'
import Modal from '@/components/Modal.vue'
import { assignmentRuleService } from '@/services/assignmentRuleService'
import { groupService } from '@/services/groupService'
import { categoryService } from '@/services/categoryService'
import userService from '@/services/userService'
import type {
  AssignmentRuleWithDetails,
  CreateAssignmentRuleRequest,
  UpdateAssignmentRuleRequest,
  AssignmentMethod
} from '@/types/assignmentRule'
import type { GroupWithMemberCount } from '@/types/group'
import type { TicketCategory } from '@/types/category'
import type { User } from '@/types/user'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

// The rule list is cached by Pinia Colada keyed here, so navigating
// away and back renders it instantly from cache and revalidates in the
// background. A skeleton shows only on the genuine first load (empty
// cache); see `isFirstLoad`.
const ASSIGNMENT_RULES_KEY = ['assignment-rules'] as const
const queryCache = useQueryCache()
const rulesQuery = useQuery({
  key: ASSIGNMENT_RULES_KEY,
  query: () => assignmentRuleService.getAllRules(),
})
const rules = computed<AssignmentRuleWithDetails[]>(() =>
  Array.isArray(rulesQuery.data.value) ? rulesQuery.data.value : (rulesQuery.data.value ?? []),
)
const isFirstLoad = computed(
  () => rulesQuery.status.value === 'pending' && rulesQuery.data.value === undefined,
)
const loadError = computed(() =>
  rulesQuery.error.value ? t('admin-assignment-rules-error-load') : '',
)

// Mutation feedback stays in local refs.
const isSaving = ref(false)
const errorMessage = ref('')
const successMessage = ref('')

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

// Method options. Labels and descriptions read through fluent so
// they re-render on locale change without remounting the view.
const methodOptions = computed<{ value: AssignmentMethod; label: string; description: string }[]>(() => [
  { value: 'direct_user', label: t('admin-assignment-rules-method-direct-label'), description: t('admin-assignment-rules-method-direct-description') },
  { value: 'group_round_robin', label: t('admin-assignment-rules-method-round-robin-label'), description: t('admin-assignment-rules-method-round-robin-description') },
  { value: 'group_random', label: t('admin-assignment-rules-method-random-label'), description: t('admin-assignment-rules-method-random-description') },
  { value: 'group_queue', label: t('admin-assignment-rules-method-queue-label'), description: t('admin-assignment-rules-method-queue-description') }
])

// Computed
const isGroupMethod = computed(() => {
  return ['group_round_robin', 'group_random', 'group_queue'].includes(ruleForm.value.method)
})

const isDirectUserMethod = computed(() => {
  return ruleForm.value.method === 'direct_user'
})

// Load supporting data
const loadSupportingData = async () => {
  try {
    const [groupsData, categoriesData, usersData] = await Promise.all([
      groupService.getGroups(),
      categoryService.getCategories(),
      userService.getPaginatedUsers({ page: 1, pageSize: 1000 })
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
    errorMessage.value = t('admin-assignment-rules-error-name')
    return
  }

  // Validate method requirements
  if (isDirectUserMethod.value && !ruleForm.value.target_user_uuid) {
    errorMessage.value = t('admin-assignment-rules-error-user')
    return
  }
  if (isGroupMethod.value && !ruleForm.value.target_group_id) {
    errorMessage.value = t('admin-assignment-rules-error-group')
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
      successMessage.value = t('admin-assignment-rules-success-update')
    } else {
      await assignmentRuleService.createRule(request)
      successMessage.value = t('admin-assignment-rules-success-create')
    }

    showRuleModal.value = false
    await queryCache.invalidateQueries({ key: ASSIGNMENT_RULES_KEY })
    setTimeout(() => (successMessage.value = ''), 3000)
  } catch (error) {
    const axiosError = error as { response?: { data?: string } }
    errorMessage.value = axiosError.response?.data || t('admin-assignment-rules-error-save')
  } finally {
    isSaving.value = false
  }
}

// Toggle rule active state
const toggleRuleActive = async (rule: AssignmentRuleWithDetails) => {
  try {
    await assignmentRuleService.updateRule(rule.id, { is_active: !rule.is_active })
    await queryCache.invalidateQueries({ key: ASSIGNMENT_RULES_KEY })
  } catch (error) {
    const axiosError = error as { response?: { data?: string } }
    errorMessage.value = axiosError.response?.data || t('admin-assignment-rules-error-update')
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
    successMessage.value = t('admin-assignment-rules-success-delete')
    showDeleteConfirm.value = false
    ruleToDelete.value = null
    await queryCache.invalidateQueries({ key: ASSIGNMENT_RULES_KEY })
    setTimeout(() => (successMessage.value = ''), 3000)
  } catch (error) {
    const axiosError = error as { response?: { data?: string } }
    errorMessage.value = axiosError.response?.data || t('admin-assignment-rules-error-delete')
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
    await queryCache.invalidateQueries({ key: ASSIGNMENT_RULES_KEY })
  } catch (error) {
    const axiosError = error as { response?: { data?: string } }
    errorMessage.value = axiosError.response?.data || t('admin-assignment-rules-error-reorder')
  }
}

// Get method display info
const getMethodInfo = (method: AssignmentMethod) => {
  return methodOptions.value.find(m => m.value === method) || { label: method, description: '' }
}

// Get target display
const getTargetDisplay = (rule: AssignmentRuleWithDetails) => {
  if (rule.method === 'direct_user' && rule.target_user) {
    return rule.target_user.name
  }
  if (rule.target_group) {
    return rule.target_group.name
  }
  return t('admin-assignment-rules-target-none')
}

onMounted(() => {
  // The rule list auto-fetches via useQuery; only the select-dropdown
  // supporting data needs an explicit load.
  loadSupportingData()
})
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-4 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <div class="mb-2 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
        <div>
          <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ $t('admin-assignment-rules-title') }}</h1>
          <p class="text-secondary mt-1">{{ $t('admin-assignment-rules-description') }}</p>
        </div>
        <button
          @click="openCreateModal"
          class="px-3 py-1.5 bg-accent text-on-accent rounded-lg text-sm hover:opacity-90 font-medium transition-colors flex items-center gap-1.5 self-start sm:self-auto"
        >
          <Icon name="add" />
          {{ $t('admin-assignment-rules-new') }}
        </button>
      </div>

      <!-- Info box -->
      <div class="bg-status-info/10 border border-status-info/30 rounded-lg p-4 text-sm text-status-info">
        <div class="flex items-start gap-2">
          <Icon name="info" size="md" class="flex-shrink-0" />
          <p>{{ $t('admin-assignment-rules-info') }}</p>
        </div>
      </div>

      <!-- Success message -->
      <AlertMessage v-if="successMessage" type="success" :message="successMessage" />

      <!-- Error message -->
      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <!-- Load error (initial fetch failed with no cached data) -->
      <AlertMessage v-if="loadError && rules.length === 0" type="error" :message="loadError" />

      <!-- First-load skeleton: mirrors the rule-card layout so the
           shell doesn't shift when data arrives. Only shown on a cold
           cache; remounts render cached rows instantly and revalidate
           silently in the background. -->
      <Skeleton
        v-if="isFirstLoad"
        :label="$t('admin-assignment-rules-loading')"
        class="flex flex-col gap-3"
      >
        <div
          v-for="n in 3"
          :key="n"
          class="bg-surface border border-default rounded-xl p-4 flex items-center gap-4"
        >
          <SkeletonBar class="w-6 h-10 rounded shrink-0" />
          <div class="flex-1 flex flex-col gap-2">
            <SkeletonBar class="h-4 w-40 max-w-full" />
            <SkeletonBar class="h-3 w-3/4" />
          </div>
        </div>
      </Skeleton>

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
                :title="$t('admin-assignment-rules-move-up')"
              >
                <Icon name="chevronUp" />
              </button>
              <span class="text-xs text-tertiary text-center w-full">{{ index + 1 }}</span>
              <button
                @click="moveRule(rule, 'down')"
                :disabled="index === rules.length - 1"
                class="p-1 text-secondary hover:text-primary hover:bg-surface-hover rounded transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
                :title="$t('admin-assignment-rules-move-down')"
              >
                <Icon name="chevronDown" />
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
                  {{ rule.is_active ? $t('admin-assignment-rules-active') : $t('admin-assignment-rules-inactive') }}
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
                  <Icon name="user" />
                  {{ getTargetDisplay(rule) }}
                </span>
                <span v-if="rule.category" class="flex items-center gap-1">
                  <Icon name="tag" />
                  {{ rule.category.name }}
                </span>
                <span class="flex items-center gap-1">
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M13 10V3L4 14h7v7l9-11h-7z" />
                  </svg>
                  <span v-if="rule.trigger_on_create && rule.trigger_on_category_change">{{ $t('admin-assignment-rules-trigger-both') }}</span>
                  <span v-else-if="rule.trigger_on_create">{{ $t('admin-assignment-rules-trigger-create') }}</span>
                  <span v-else-if="rule.trigger_on_category_change">{{ $t('admin-assignment-rules-trigger-category') }}</span>
                  <span v-else>{{ $t('admin-assignment-rules-trigger-none') }}</span>
                </span>
                <span v-if="rule.state" class="flex items-center gap-1">
                  <Icon name="insights" />
                  {{ t('admin-assignment-rules-assigned-count', { count: rule.state.total_assignments }) }}
                </span>
              </div>
            </div>

            <!-- Actions -->
            <div class="flex items-center gap-2 flex-shrink-0">
              <button
                @click="toggleRuleActive(rule)"
                class="p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
                :title="rule.is_active ? $t('admin-assignment-rules-toggle-deactivate') : $t('admin-assignment-rules-toggle-activate')"
              >
                <Icon v-if="rule.is_active" name="close" />
                <Icon v-else name="checkCircle" />
              </button>
              <button
                @click="openEditModal(rule)"
                class="p-2 text-secondary hover:text-primary hover:bg-surface-hover rounded-lg transition-colors"
                :title="$t('admin-assignment-rules-edit')"
              >
                <Icon name="rename" />
              </button>
              <button
                @click="confirmDelete(rule)"
                class="p-2 text-secondary hover:text-status-error hover:bg-status-error/10 rounded-lg transition-colors"
                :title="$t('admin-assignment-rules-delete')"
              >
                <Icon name="trash" />
              </button>
            </div>
          </div>
        </div>

        <!-- Empty state -->
        <EmptyState
          v-if="rules.length === 0 && !isFirstLoad"
          icon="ticket"
          :title="$t('empty-assignment-rules-title')"
          :description="$t('empty-assignment-rules-description')"
          :action-label="$t('admin-assignment-rules-create-action')"
          variant="card"
          @action="openCreateModal"
        />
      </div>
    </div>

    <!-- Create/Edit Rule Modal -->
    <Modal
      :show="showRuleModal"
      :title="editingRule ? $t('admin-assignment-rules-modal-edit-title') : $t('admin-assignment-rules-modal-create-title')"
      size="lg"
      @close="showRuleModal = false"
    >
      <form @submit.prevent="saveRule" class="flex flex-col gap-4">
        <!-- Name -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-assignment-rules-modal-name-label') }}</label>
          <input
            v-model="ruleForm.name"
            type="text"
            class="w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent"
            :placeholder="$t('admin-assignment-rules-modal-name-placeholder')"
          />
        </div>

        <!-- Description -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-assignment-rules-modal-description-label') }}</label>
          <textarea
            v-model="ruleForm.description"
            rows="2"
            class="w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent resize-none"
            :placeholder="$t('admin-assignment-rules-modal-description-placeholder')"
          ></textarea>
        </div>

        <!-- Assignment Method -->
        <div>
          <label class="block text-sm font-medium text-primary mb-2">{{ $t('admin-assignment-rules-modal-method-label') }}</label>
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
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
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-assignment-rules-modal-user-label') }}</label>
          <select
            v-model="ruleForm.target_user_uuid"
            class="w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent"
          >
            <option :value="undefined">{{ $t('admin-assignment-rules-modal-user-placeholder') }}</option>
            <option v-for="user in users" :key="user.uuid" :value="user.uuid">
              {{ user.name }}
            </option>
          </select>
        </div>

        <!-- Target Group (for group methods) -->
        <div v-if="isGroupMethod">
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-assignment-rules-modal-group-label') }}</label>
          <select
            v-model="ruleForm.target_group_id"
            class="w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent"
          >
            <option :value="undefined">{{ $t('admin-assignment-rules-modal-group-placeholder') }}</option>
            <option v-for="group in groups" :key="group.id" :value="group.id">
              {{ group.name }} ({{ t('admin-assignment-rules-modal-group-members', { count: group.member_count }) }})
            </option>
          </select>
        </div>

        <!-- Category Filter -->
        <div>
          <label class="block text-sm font-medium text-primary mb-1">{{ $t('admin-assignment-rules-modal-category-label') }}</label>
          <select
            v-model="ruleForm.category_id"
            class="w-full px-3 py-2 bg-surface border border-default rounded-lg text-primary focus:outline-none focus:ring-2 focus:ring-accent"
          >
            <option :value="undefined">{{ $t('admin-assignment-rules-modal-category-all') }}</option>
            <option v-for="category in categories" :key="category.id" :value="category.id">
              {{ category.name }}
            </option>
          </select>
          <p class="text-xs text-tertiary mt-1">{{ $t('admin-assignment-rules-modal-category-hint') }}</p>
        </div>

        <!-- Triggers -->
        <div>
          <label class="block text-sm font-medium text-primary mb-2">{{ $t('admin-assignment-rules-modal-triggers-label') }}</label>
          <div class="flex flex-col gap-3">
            <Checkbox
              :model-value="ruleForm.trigger_on_create ?? false"
              @update:model-value="ruleForm.trigger_on_create = $event"
              :label="$t('admin-assignment-rules-modal-trigger-create-label')"
            />
            <Checkbox
              :model-value="ruleForm.trigger_on_category_change ?? false"
              @update:model-value="ruleForm.trigger_on_category_change = $event"
              :label="$t('admin-assignment-rules-modal-trigger-category-label')"
            />
          </div>
        </div>

        <!-- Active toggle -->
        <div class="flex items-center justify-between pt-2">
          <span class="text-sm text-secondary">{{ $t('admin-assignment-rules-modal-active-label') }}</span>
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
            {{ $t('admin-assignment-rules-modal-cancel') }}
          </button>
          <button
            @click="saveRule"
            :disabled="isSaving"
            class="px-4 py-2 bg-accent text-on-accent rounded-lg hover:opacity-90 transition-colors disabled:opacity-50"
          >
            {{ isSaving ? $t('admin-assignment-rules-modal-saving') : editingRule ? $t('admin-assignment-rules-modal-update') : $t('admin-assignment-rules-modal-create') }}
          </button>
        </div>
      </template>
    </Modal>

    <!-- Delete Confirmation Modal -->
    <Modal
      :show="showDeleteConfirm"
      :title="$t('admin-assignment-rules-delete-title')"
      size="sm"
      @close="showDeleteConfirm = false"
    >
      <p class="text-secondary">
        {{ t('admin-assignment-rules-delete-message', { name: ruleToDelete?.name ?? '' }) }}
      </p>

      <template #footer>
        <div class="flex justify-end gap-3">
          <button
            @click="showDeleteConfirm = false"
            class="px-4 py-2 text-secondary hover:text-primary transition-colors"
          >
            {{ $t('admin-assignment-rules-delete-cancel') }}
          </button>
          <button
            @click="deleteRule"
            :disabled="isSaving"
            class="px-4 py-2 bg-status-error text-white rounded-lg hover:opacity-90 transition-colors disabled:opacity-50"
          >
            {{ isSaving ? $t('admin-assignment-rules-deleting') : $t('admin-assignment-rules-delete-confirm') }}
          </button>
        </div>
      </template>
    </Modal>
  </div>
</template>
