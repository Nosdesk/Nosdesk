/**
 * Ticket detail sync facade.
 *
 * Pool-native view-model for the ticket detail page. Everything the
 * page renders is derived from the sync object pool (fed by the
 * `ticket:<id>` bootstrap + the live sync stream), so the detail view
 * shares one real-time substrate with the list / board views instead
 * of a parallel REST + discrete-SSE encoding.
 *
 * Reads: the ticket row, its comments + attachments, device links
 * (`ticket_asset`), ticket links (`linked_ticket`), and project
 * memberships (`project_ticket`) come straight from the pool;
 * requester / assignee / comment authors resolve through the user
 * roster, and the cycle chip through the lazy `cycle` reference.
 *
 * Writes: scalar ticket fields go through `dispatchOptimistic` (the
 * crash-safe push queue, same path the board uses). Tags / watch and
 * the relation mutations (comment, link, asset, project) keep their
 * dedicated REST endpoints — those emit the `sync_actions` that flow
 * back through the pool — with an optimistic pool mutation for instant
 * feedback and an inverse on failure.
 *
 * Folds what `useTicketData` / `useTicketComments` /
 * `useTicketRelationships` / `useTicketAssets` / `useTicketMutations`
 * used to do, minus the REST-fetch-then-mutate-a-local-ref model.
 */
import { computed, ref, toValue, type MaybeRefOrGetter } from 'vue'
import { useRouter } from 'vue-router'
import { useEntity, useReference, useAggregate } from '@/sync/composables'
import * as pool from '@/sync/pool'
import { dispatchOptimistic } from '@/sync/queue'
import { useWorkflowStatesStore } from '@/stores/workflowStates'
import { useRecentTicketsStore } from '@/stores/recentTickets'
import { logger } from '@/utils/logger'
import { formatDateTime } from '@nosdesk/core/utils/dateUtils'
import ticketService, { getCommentsByTicketId } from '@/services/ticketService'
import apiClient from '@nosdesk/core/apiClient'
import { projectService } from '@/services/projectService'
import type { TicketPriority } from '@nosdesk/core/constants/ticketOptions'
import type { CardWorkflowState } from '@/sync/views/types'
import type { Asset } from '@nosdesk/core/types/asset'
import type { Project } from '@nosdesk/core/types/project'
import type { CommentWithAttachments, Attachment } from '@nosdesk/core/types/comment'
import type { PlatformRole } from '@nosdesk/core/types/user'
import type { WorkspaceRole } from '@nosdesk/core/types/workspace'
import type { TicketCategory } from '@nosdesk/core/types/category'

/** Ticket row as it lands in the pool for the detail view: the board
 * `SyncTicket` plus the detail-only scalars Stage 1 added to the
 * bootstrap + `ticket.updated` emit. */
interface SyncTicketDetail {
  id: number
  title: string
  workflow_state: CardWorkflowState | null
  workflow_state_id: number
  priority: TicketPriority
  requester_uuid: string | null
  assignee_uuid: string | null
  category_id: number | null
  due_date: string | null
  recurrence_rule: string | null
  resolution_notes: string | null
  tag_ids?: number[]
  watcher_uuids?: string[]
  cycle_id?: number | null
  sla?: import('@nosdesk/core/types/sla').SlaPill | null
  created_by?: string | null
  closed_by?: string | null
  closed_at?: string | null
  submitted_via?: string | null
  origin_channel_id?: number | null
  merged_into_ticket_id?: number | null
  merged_at?: string | null
  merged_by_user_uuid?: string | null
  spam_suspected?: boolean
  created_at: string
  updated_at: string
}

interface PoolComment {
  id: number
  ticket_id: number
  user_uuid: string
  content: string
  is_internal: boolean
  content_format?: string
  // Render tier (see CommentRenderKind). Carried through the pool so
  // CommentContent picks inline vs iframe without a REST round-trip;
  // absent on legacy rows, where the renderer falls back to format.
  render_kind?: string | null
  created_at: string
}

interface PoolAttachment {
  id: number
  comment_id: number | null
  name: string
  url: string
  mime_type?: string | null
  file_size?: number | null
}

interface TicketAssetRow {
  ticket_id: number
  asset_id: number
}
interface LinkedTicketRow {
  ticket_id: number
  linked_ticket_id: number
}
interface ProjectTicketRow {
  project_id: number
  ticket_id: number
}

interface PoolUser {
  uuid: string
  name: string
  email?: string
  platform_role?: string
  workspace_role?: string | null
  avatar_url?: string | null
  avatar_thumb?: string | null
}

interface UploadedFile {
  id: number
  url: string
  name: string
  transcription?: string
}

interface FileWithTranscription extends File {
  _transcription?: string
}

export function useTicketDetail(
  ticketIdRef: MaybeRefOrGetter<number | undefined>,
  /** Workspace ticket categories (reference data loaded by the view),
   * used to resolve the category chip object from `category_id`. */
  categories?: MaybeRefOrGetter<TicketCategory[]>,
) {
  const router = useRouter()
  const workflowStatesStore = useWorkflowStatesStore()
  const recentTicketsStore = useRecentTicketsStore()

  const id = computed<number | null>(() => {
    const v = toValue(ticketIdRef)
    return typeof v === 'number' && Number.isFinite(v) ? v : null
  })

  const row = useEntity<SyncTicketDetail>('ticket', () => id.value)
  const requesterUser = useReference<PoolUser>('user', () => row.value?.requester_uuid ?? null)
  const assigneeUser = useReference<PoolUser>('user', () => row.value?.assignee_uuid ?? null)
  const cycle = useReference<{ id: number; uuid: string; project_id: number; name: string; state: string }>(
    'cycle',
    () => row.value?.cycle_id ?? null,
  )

  const commentAgg = useAggregate<PoolComment>('comment')
  const attachmentAgg = useAggregate<PoolAttachment>('attachment')
  const ticketAssetAgg = useAggregate<TicketAssetRow>('ticket_asset')
  const linkedAgg = useAggregate<LinkedTicketRow>('linked_ticket')
  const projectTicketAgg = useAggregate<ProjectTicketRow>('project_ticket')

  // -------------------- reads --------------------

  const linkedTickets = computed<number[]>(() =>
    id.value == null
      ? []
      : linkedAgg.value.filter((l) => l.ticket_id === id.value).map((l) => l.linked_ticket_id),
  )

  const projects = computed<string[]>(() =>
    id.value == null
      ? []
      : projectTicketAgg.value.filter((p) => p.ticket_id === id.value).map((p) => String(p.project_id)),
  )

  const devices = computed<Asset[]>(() => {
    if (id.value == null) return []
    return ticketAssetAgg.value
      .filter((ta) => ta.ticket_id === id.value)
      .map((ta) => pool.get<Asset>('asset', ta.asset_id))
      .filter((a): a is Asset => a != null)
  })

  const categoryObject = computed<TicketCategory | null>(() => {
    const cid = row.value?.category_id
    if (cid == null) return null
    const list = toValue(categories) ?? []
    return list.find((c) => c.id === cid) ?? null
  })

  /** Assembled object matching the shape `TicketDetails.vue`,
   * `CommentsAndAttachments.vue`, and the modals already consume, so
   * the child contracts don't change — only the data source. */
  const ticket = computed(() => {
    const r = row.value
    if (!r) return null
    return {
      id: r.id,
      title: r.title,
      priority: r.priority,
      created: r.created_at,
      modified: r.updated_at,
      requester: r.requester_uuid ?? '',
      assignee: r.assignee_uuid ?? '',
      requester_user: requesterUser.value,
      assignee_user: assigneeUser.value,
      category_id: r.category_id,
      category: categoryObject.value,
      origin_channel_id: r.origin_channel_id ?? null,
      submitted_via: r.submitted_via ?? null,
      due_date: r.due_date,
      recurrence_rule: r.recurrence_rule,
      created_by: r.created_by ?? null,
      closed_by: r.closed_by ?? null,
      closed_at: r.closed_at ?? null,
      sla: r.sla ?? null,
      cycle: cycle.value,
      resolution_notes: r.resolution_notes,
      tag_ids: r.tag_ids ?? [],
      watcher_uuids: r.watcher_uuids ?? [],
      workflow_state: r.workflow_state,
      workflow_state_id: r.workflow_state_id,
      projects: projects.value,
      linkedTickets: linkedTickets.value,
      merged_into_ticket_id: r.merged_into_ticket_id ?? null,
      merged_at: r.merged_at ?? null,
      spam_suspected: r.spam_suspected ?? false,
      merged_by_user_uuid: r.merged_by_user_uuid ?? null,
    }
  })

  const comments = computed<CommentWithAttachments[]>(() => {
    if (id.value == null) return []
    return commentAgg.value
      .filter((c) => c.ticket_id === id.value)
      .map((c) => {
        const author = pool.get<PoolUser>('user', c.user_uuid)
        const attachments = attachmentAgg.value
          .filter((a) => a.comment_id === c.id)
          .map(
            (a): Attachment => ({
              id: a.id,
              url: a.url,
              name: a.name,
              comment_id: a.comment_id ?? c.id,
              mime_type: a.mime_type ?? undefined,
              file_size: a.file_size ?? undefined,
            }),
          )
        return {
          id: c.id,
          content: c.content,
          content_format: c.content_format as CommentWithAttachments['content_format'],
          render_kind: c.render_kind as CommentWithAttachments['render_kind'],
          user_uuid: c.user_uuid,
          created_at: c.created_at,
          createdAt: c.created_at,
          ticket_id: c.ticket_id,
          is_internal: c.is_internal,
          attachments,
          user: author
            ? {
                uuid: author.uuid,
                name: author.name,
                email: author.email ?? '',
                platform_role: (author.platform_role ?? 'user') as PlatformRole,
                workspace_role: (author.workspace_role ?? null) as WorkspaceRole | null,
                avatar_url: author.avatar_url ?? null,
                avatar_thumb: author.avatar_thumb ?? null,
              }
            : undefined,
        } as CommentWithAttachments
      })
      .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
  })

  const selectedPriority = computed<TicketPriority>(() => row.value?.priority ?? 'low')
  const selectedCategory = computed<number | null>(() => row.value?.category_id ?? null)
  const selectedWorkflowStateId = computed<number | null>(() => row.value?.workflow_state_id ?? null)
  const formattedCreatedDate = computed(() => formatDateTime(row.value?.created_at))
  const formattedModifiedDate = computed(() => formatDateTime(row.value?.updated_at))

  // -------------------- scalar field writes (push queue) --------------------

  /** Optimistic patch of scalar ticket fields via the crash-safe push
   * queue. `forward` lands in the pool immediately; `inverse` restores
   * it if the server rejects. No-op when the row isn't loaded. */
  async function patchTicket(
    forward: Partial<SyncTicketDetail>,
    inverse: Partial<SyncTicketDetail>,
  ): Promise<void> {
    if (id.value == null) return
    await dispatchOptimistic<SyncTicketDetail>('ticket', id.value, { forward, inverse })
  }

  async function updateWorkflowState(newId: number): Promise<void> {
    const r = row.value
    if (!r || r.workflow_state_id === newId) return
    const target = workflowStatesStore.findById(newId)
    await patchTicket(
      {
        workflow_state_id: newId,
        workflow_state: (target as CardWorkflowState | undefined) ?? r.workflow_state,
      },
      { workflow_state_id: r.workflow_state_id, workflow_state: r.workflow_state },
    )
    if (id.value != null) {
      recentTicketsStore.updateTicketData(id.value, { workflow_state_id: newId })
    }
  }

  async function updatePriority(priority: TicketPriority): Promise<void> {
    const r = row.value
    if (!r || r.priority === priority) return
    await patchTicket({ priority }, { priority: r.priority })
  }

  async function updateCategory(categoryStr: string): Promise<void> {
    const r = row.value
    if (!r) return
    const categoryId = categoryStr ? parseInt(categoryStr, 10) : null
    if (r.category_id === categoryId) return
    await patchTicket({ category_id: categoryId }, { category_id: r.category_id })
  }

  async function updateRequester(uuid: string): Promise<void> {
    const r = row.value
    if (!r) return
    const next = uuid || null
    if (r.requester_uuid === next) return
    await patchTicket({ requester_uuid: next }, { requester_uuid: r.requester_uuid })
  }

  async function updateAssignee(uuid: string): Promise<void> {
    const r = row.value
    if (!r) return
    const next = uuid || null
    if (r.assignee_uuid === next) return
    await patchTicket({ assignee_uuid: next }, { assignee_uuid: r.assignee_uuid })
    if (id.value != null) recentTicketsStore.updateTicketData(id.value, { assignee: next ?? '' })
  }

  async function updateTitle(title: string): Promise<void> {
    const r = row.value
    if (!r || r.title === title) return
    await patchTicket({ title }, { title: r.title })
    if (id.value != null) recentTicketsStore.updateTicketData(id.value, { title })
  }

  async function updateDueDate(due: string | null): Promise<void> {
    const r = row.value
    if (!r || r.due_date === due) return
    await patchTicket({ due_date: due }, { due_date: r.due_date })
  }

  async function updateRecurrenceRule(rule: string | null): Promise<void> {
    const r = row.value
    if (!r || r.recurrence_rule === rule) return
    await patchTicket({ recurrence_rule: rule }, { recurrence_rule: r.recurrence_rule })
  }

  async function updateResolutionNotes(value: string | null): Promise<void> {
    const r = row.value
    if (!r) return
    const normalised = value && value.trim().length > 0 ? value : null
    if (r.resolution_notes === normalised) return
    await patchTicket({ resolution_notes: normalised }, { resolution_notes: r.resolution_notes })
  }

  // -------------------- tags / watch (dedicated endpoints) --------------------

  async function updateTags(tagIds: number[]): Promise<void> {
    if (id.value == null) return
    const r = row.value
    const previous = r?.tag_ids ?? []
    pool.patch<SyncTicketDetail>('ticket', id.value, { tag_ids: tagIds })
    try {
      const { tagService } = await import('@/services/tagService')
      const next = await tagService.setForTicket(id.value, tagIds)
      pool.patch<SyncTicketDetail>('ticket', id.value, { tag_ids: next })
    } catch (err) {
      logger.error('Failed to update tags', { error: err })
      pool.patch<SyncTicketDetail>('ticket', id.value, { tag_ids: previous })
    }
  }

  async function toggleWatch(currentUserUuid: string): Promise<void> {
    if (id.value == null) return
    const r = row.value
    const current = r?.watcher_uuids ?? []
    const isWatching = current.includes(currentUserUuid)
    const optimistic = isWatching
      ? current.filter((u) => u !== currentUserUuid)
      : [...current, currentUserUuid]
    pool.patch<SyncTicketDetail>('ticket', id.value, { watcher_uuids: optimistic })
    try {
      const { watcherService } = await import('@/services/watcherService')
      if (isWatching) await watcherService.unwatch(id.value)
      else await watcherService.watch(id.value)
    } catch (err) {
      logger.error('Failed to toggle watch', { error: err })
      pool.patch<SyncTicketDetail>('ticket', id.value, { watcher_uuids: current })
    }
  }

  async function deleteTicket(): Promise<void> {
    if (id.value == null) return
    await ticketService.deleteTicket(id.value)
    router.push('/tickets')
  }

  // -------------------- comments --------------------

  const recentlyAddedCommentIds = ref<Set<number>>(new Set())
  function highlightComment(commentId: number): void {
    recentlyAddedCommentIds.value.add(commentId)
    setTimeout(() => recentlyAddedCommentIds.value.delete(commentId), 3000)
  }

  async function addComment(data: {
    content: string
    user_uuid: string
    files: File[]
    is_internal?: boolean
  }): Promise<void> {
    if (id.value == null) return
    if (!data.content.trim() && (!data.files || data.files.length === 0)) return

    const ticketId = id.value
    const tempId = -Date.now()
    const nowIso = new Date().toISOString()
    // Optimistic comment row in the pool. The real `comment.created`
    // sync action reconciles it (or the REST response below does, if
    // sync is slow).
    pool.upsert<PoolComment>('comment', tempId, {
      id: tempId,
      ticket_id: ticketId,
      user_uuid: data.user_uuid,
      content: data.content,
      is_internal: data.is_internal === true,
      // Match the tier the backend stamps on UI-authored comments so the
      // optimistic bubble renders inline, identical to the reconciled row.
      content_format: 'html',
      render_kind: 'simple',
      created_at: nowIso,
    })

    try {
      let attachments: UploadedFile[] = []
      if (data.files?.length > 0) {
        const formData = new FormData()
        const audioFile = data.files.find((f) => f.type.startsWith('audio/')) as
          | FileWithTranscription
          | undefined
        if (audioFile?._transcription) {
          formData.append('transcription', audioFile._transcription)
        }
        data.files.forEach((file) => formData.append('files', file, file.name))
        const response = await apiClient.post<UploadedFile[]>('/upload', formData, {
          headers: { 'Content-Type': 'multipart/form-data' },
        })
        attachments = response.data.map((f) => ({
          id: f.id,
          url: f.url,
          name: f.name,
          transcription: f.transcription,
        }))
      }

      const newComment = await ticketService.addCommentToTicket(
        ticketId,
        data.content,
        attachments,
        data.is_internal === true,
      )

      // Drop the optimistic temp row; upsert the authoritative comment
      // (idempotent with the incoming sync action) and its attachments.
      pool.remove('comment', tempId)
      pool.upsert<PoolComment>('comment', newComment.id, {
        id: newComment.id,
        ticket_id: newComment.ticket_id,
        user_uuid: newComment.user_uuid,
        content: newComment.content,
        is_internal: data.is_internal === true,
        content_format: newComment.content_format,
        render_kind: newComment.render_kind,
        created_at: newComment.created_at,
      })
      for (const a of newComment.attachments ?? []) {
        pool.upsert<PoolAttachment>('attachment', a.id, {
          id: a.id,
          comment_id: newComment.id,
          name: a.name,
          url: a.url,
          mime_type: a.mime_type ?? null,
          file_size: a.file_size ?? null,
        })
      }
      highlightComment(newComment.id)
    } catch (err) {
      logger.error('Error adding comment', { ticketId, error: err })
      pool.remove('comment', tempId)
    }
  }

  async function deleteComment(commentId: number): Promise<void> {
    const snapshot = pool.get<PoolComment>('comment', commentId)
    pool.remove('comment', commentId)
    try {
      await ticketService.deleteComment(commentId)
    } catch (err) {
      logger.error('Error deleting comment', { commentId, error: err })
      if (snapshot) pool.upsert<PoolComment>('comment', commentId, { ...snapshot })
    }
  }

  async function deleteAttachment(data: {
    commentId: number
    attachmentIndex: number
  }): Promise<void> {
    const att = attachmentAgg.value
      .filter((a) => a.comment_id === data.commentId)
      .sort((a, b) => a.id - b.id)[data.attachmentIndex]
    if (!att) return
    const comment = pool.get<PoolComment>('comment', data.commentId)
    const siblings = attachmentAgg.value.filter((a) => a.comment_id === data.commentId)
    const hasNoRealContent =
      !comment?.content ||
      comment.content.trim() === '' ||
      comment.content.trim().toLowerCase() === 'attachment added'
    if (siblings.length === 1 && hasNoRealContent) {
      await deleteComment(data.commentId)
      return
    }
    const snapshot = pool.get<PoolAttachment>('attachment', att.id)
    pool.remove('attachment', att.id)
    try {
      await ticketService.deleteAttachment(att.id)
    } catch (err) {
      logger.error('Error deleting attachment', { error: err })
      if (snapshot) pool.upsert<PoolAttachment>('attachment', att.id, { ...snapshot })
    }
  }

  /** Refresh comments from REST into the pool. Used as a fallback by
   * the merge-marker path (the destination's marker comment arrives as
   * a pool row, but a belt-and-braces refetch covers a slow stream). */
  async function refreshComments(): Promise<void> {
    if (id.value == null) return
    try {
      const fresh = await getCommentsByTicketId(id.value)
      for (const c of fresh) {
        pool.upsert<PoolComment>('comment', c.id, {
          id: c.id,
          ticket_id: c.ticket_id,
          user_uuid: c.user_uuid,
          content: c.content,
          is_internal: c.is_internal === true,
          content_format: c.content_format,
          render_kind: c.render_kind,
          created_at: c.created_at,
        })
        for (const a of c.attachments ?? []) {
          pool.upsert<PoolAttachment>('attachment', a.id, {
            id: a.id,
            comment_id: c.id,
            name: a.name,
            url: a.url,
            mime_type: a.mime_type ?? null,
            file_size: a.file_size ?? null,
          })
        }
      }
    } catch {
      // Best-effort; the next delta picks it up.
    }
  }

  // -------------------- relationships (links / projects) --------------------

  const showLinkedTicketModal = ref(false)
  const showProjectModal = ref(false)

  function linkedKey(a: number, b: number): string {
    return `${a}:${b}`
  }

  async function linkTicket(linkedTicketId: number): Promise<void> {
    if (id.value == null || linkedTickets.value.includes(linkedTicketId)) return
    const ticketId = id.value
    pool.upsert<LinkedTicketRow>('linked_ticket', linkedKey(ticketId, linkedTicketId), {
      ticket_id: ticketId,
      linked_ticket_id: linkedTicketId,
    })
    try {
      await ticketService.linkTicket(ticketId, linkedTicketId)
    } catch (err) {
      logger.error('Error linking ticket', { error: err })
      pool.remove('linked_ticket', linkedKey(ticketId, linkedTicketId))
    }
  }

  async function unlinkTicket(linkedTicketId: number): Promise<void> {
    if (id.value == null) return
    const ticketId = id.value
    pool.remove('linked_ticket', linkedKey(ticketId, linkedTicketId))
    try {
      await ticketService.unlinkTicket(ticketId, linkedTicketId)
    } catch (err) {
      logger.error('Error unlinking ticket', { error: err })
      pool.upsert<LinkedTicketRow>('linked_ticket', linkedKey(ticketId, linkedTicketId), {
        ticket_id: ticketId,
        linked_ticket_id: linkedTicketId,
      })
    }
  }

  async function addToProject(project: Project): Promise<void> {
    showProjectModal.value = false
    if (id.value == null || projects.value.includes(String(project.id))) return
    const ticketId = id.value
    const projectId = Number(project.id)
    pool.upsert<ProjectTicketRow>('project_ticket', linkedKey(projectId, ticketId), {
      project_id: projectId,
      ticket_id: ticketId,
    })
    try {
      await projectService.addTicketToProject(project.id, ticketId)
    } catch (err) {
      logger.error('Error adding ticket to project', { error: err })
      pool.remove('project_ticket', linkedKey(projectId, ticketId))
    }
  }

  async function removeFromProject(projectId: string): Promise<void> {
    if (id.value == null) return
    const ticketId = id.value
    const pid = Number(projectId)
    pool.remove('project_ticket', linkedKey(pid, ticketId))
    try {
      await projectService.removeTicketFromProject(pid, ticketId)
    } catch (err) {
      logger.error('Error removing ticket from project', { error: err })
      pool.upsert<ProjectTicketRow>('project_ticket', linkedKey(pid, ticketId), {
        project_id: pid,
        ticket_id: ticketId,
      })
    }
  }

  // -------------------- devices (assets) --------------------

  const showDeviceModal = ref(false)

  async function addDevice(device: Asset): Promise<void> {
    showDeviceModal.value = false
    if (id.value == null || devices.value.some((d) => d.id === device.id)) return
    const ticketId = id.value
    // Ensure the asset row is in the pool so the chip resolves before
    // the bootstrap roster catches a brand-new asset.
    pool.upsert<Asset>('asset', device.id, device)
    pool.upsert<TicketAssetRow>('ticket_asset', linkedKey(ticketId, device.id), {
      ticket_id: ticketId,
      asset_id: device.id,
    })
    try {
      await ticketService.addDeviceToTicket(ticketId, device.id)
    } catch (err) {
      logger.error('Error adding device to ticket', { error: err })
      pool.remove('ticket_asset', linkedKey(ticketId, device.id))
    }
  }

  async function removeDevice(deviceId: number): Promise<void> {
    if (id.value == null) return
    const ticketId = id.value
    pool.remove('ticket_asset', linkedKey(ticketId, deviceId))
    try {
      await ticketService.removeDeviceFromTicket(ticketId, deviceId)
    } catch (err) {
      logger.error('Error removing device from ticket', { error: err })
      pool.upsert<TicketAssetRow>('ticket_asset', linkedKey(ticketId, deviceId), {
        ticket_id: ticketId,
        asset_id: deviceId,
      })
    }
  }

  /** Record a recents entry for this ticket (sidebar "recently viewed"). */
  function recordView(): void {
    if (id.value != null) void recentTicketsStore.recordTicketView(id.value)
  }

  return {
    // reads
    ticket,
    comments,
    devices,
    selectedPriority,
    selectedCategory,
    selectedWorkflowStateId,
    formattedCreatedDate,
    formattedModifiedDate,
    recentlyAddedCommentIds,
    // scalar writes
    updateWorkflowState,
    updatePriority,
    updateCategory,
    updateRequester,
    updateAssignee,
    updateTitle,
    updateDueDate,
    updateRecurrenceRule,
    updateResolutionNotes,
    updateTags,
    toggleWatch,
    deleteTicket,
    // comments
    addComment,
    deleteComment,
    deleteAttachment,
    refreshComments,
    // relationships
    showLinkedTicketModal,
    showProjectModal,
    linkTicket,
    unlinkTicket,
    addToProject,
    removeFromProject,
    // devices
    showDeviceModal,
    addDevice,
    removeDevice,
    // misc
    recordView,
  } as const
}

export type UseTicketDetail = ReturnType<typeof useTicketDetail>
