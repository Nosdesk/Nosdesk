import { ref, type Ref } from 'vue';
import ticketService from '@/services/ticketService';
import { projectService } from '@/services/projectService';
import type { Project } from '@/types/project';
import type { Ticket } from '@/types/ticket';
import { useTicketMutations } from './useTicketMutations';

/**
 * Composable for managing ticket relationships (linked tickets and projects)
 *
 * Uses centralized mutations from useTicketMutations for consistent
 * array operations with built-in deduplication.
 */
export function useTicketRelationships(ticket: Ref<Ticket | null>) {
  const mutations = useTicketMutations(ticket);
  const showLinkedTicketModal = ref(false);
  const showProjectModal = ref(false);

  // Link ticket with optimistic update
  async function linkTicket(linkedTicketId: number): Promise<void> {
    if (!ticket.value || mutations.hasLinkedTicket(linkedTicketId)) return;

    // Optimistic update
    mutations.addLinkedTicket(linkedTicketId);

    try {
      await ticketService.linkTicket(ticket.value.id, linkedTicketId);
    } catch (err) {
      console.error('Error linking ticket:', err);
      mutations.removeLinkedTicket(linkedTicketId);
    }
  }

  // Unlink ticket with optimistic update
  async function unlinkTicket(linkedTicketId: number): Promise<void> {
    if (!ticket.value || !mutations.hasLinkedTicket(linkedTicketId)) return;

    // Optimistic update
    mutations.removeLinkedTicket(linkedTicketId);

    try {
      await ticketService.unlinkTicket(ticket.value.id, linkedTicketId);
    } catch (err) {
      console.error('Error unlinking ticket:', err);
      mutations.addLinkedTicket(linkedTicketId);
    }
  }

  // Add ticket to project with optimistic update
  async function addToProject(project: Project): Promise<void> {
    if (!ticket.value || mutations.hasProject(project.id)) {
      showProjectModal.value = false;
      return;
    }

    // Optimistic update
    mutations.addProject(project.id);
    showProjectModal.value = false;

    try {
      await projectService.addTicketToProject(project.id, ticket.value.id);
    } catch (err) {
      console.error('Error adding ticket to project:', err);
      mutations.removeProject(project.id);
    }
  }

  // Remove ticket from project with optimistic update
  async function removeFromProject(projectId: string): Promise<void> {
    if (!ticket.value || !mutations.hasProject(projectId)) return;

    // Optimistic update
    mutations.removeProject(projectId);

    try {
      await projectService.removeTicketFromProject(Number(projectId), ticket.value.id);
    } catch (err) {
      console.error('Error removing ticket from project:', err);
      mutations.addProject(projectId);
    }
  }

  return {
    showLinkedTicketModal,
    showProjectModal,
    linkTicket,
    unlinkTicket,
    addToProject,
    removeFromProject,
  };
}
