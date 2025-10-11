import { ref, type Ref } from 'vue';
import ticketService from '@/services/ticketService';
import { projectService } from '@/services/projectService';
import type { Project } from '@/types/project';

/**
 * Composable for managing ticket relationships (linked tickets and projects)
 */
export function useTicketRelationships(ticket: Ref<any>, refreshTicket: () => Promise<void>) {
  const showLinkedTicketModal = ref(false);
  const showProjectModal = ref(false);

  // Link ticket
  async function linkTicket(linkedTicketId: number): Promise<void> {
    if (!ticket.value) return;

    if (!ticket.value.linkedTickets) {
      ticket.value.linkedTickets = [];
    }

    if (ticket.value.linkedTickets.includes(linkedTicketId)) {
      return;
    }

    try {
      await ticketService.linkTicket(ticket.value.id, linkedTicketId);
      await refreshTicket();
    } catch (err) {
      console.error('Error linking ticket:', err);
    }
  }

  // Unlink ticket
  async function unlinkTicket(linkedTicketId: number): Promise<void> {
    if (!ticket.value) return;

    try {
      await ticketService.unlinkTicket(ticket.value.id, linkedTicketId);
      await refreshTicket();
    } catch (err) {
      console.error('Error unlinking ticket:', err);
    }
  }

  // Add ticket to project
  async function addToProject(project: Project): Promise<void> {
    if (!ticket.value) return;

    // Check if already in this project
    if (ticket.value.projects?.includes(String(project.id))) {
      showProjectModal.value = false;
      return;
    }

    try {
      await projectService.addTicketToProject(project.id, ticket.value.id);
      await refreshTicket();
      showProjectModal.value = false;
    } catch (err) {
      console.error('Error adding ticket to project:', err);
    }
  }

  // Remove ticket from project
  async function removeFromProject(projectId: string): Promise<void> {
    if (!ticket.value?.projects) return;

    try {
      await projectService.removeTicketFromProject(Number(projectId), ticket.value.id);
      await refreshTicket();
    } catch (err) {
      console.error('Error removing ticket from project:', err);
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
