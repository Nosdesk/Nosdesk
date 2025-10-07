import { ref, type Ref } from 'vue';
import ticketService from '@/services/ticketService';
import { projectService } from '@/services/projectService';

/**
 * Composable for managing ticket relationships (linked tickets and projects)
 */
export function useTicketRelationships(ticket: Ref<any>, refreshTicket: () => Promise<void>) {
  const showLinkedTicketModal = ref(false);
  const showProjectModal = ref(false);
  const projectDetails = ref<any>(null);

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
  async function addToProject(projectId: number): Promise<void> {
    if (!ticket.value) return;

    try {
      await projectService.addTicketToProject(projectId, ticket.value.id);

      const project = await projectService.getProject(projectId);
      projectDetails.value = {
        id: project.id,
        name: project.name,
        description: project.description,
        status: project.status,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        ticket_count: project.ticketCount || 0,
      };

      ticket.value.project = String(projectId);
      showProjectModal.value = false;

      await refreshTicket();
    } catch (err) {
      console.error('Error adding ticket to project:', err);
    }
  }

  // Remove ticket from project
  async function removeFromProject(): Promise<void> {
    if (!ticket.value?.project) return;

    try {
      const projectId = Number(ticket.value.project);
      await projectService.removeTicketFromProject(projectId, ticket.value.id);

      ticket.value.project = undefined;
      projectDetails.value = null;
    } catch (err) {
      console.error('Error removing ticket from project:', err);
    }
  }

  // Fetch project details
  async function fetchProjectDetails(projectId: string): Promise<void> {
    try {
      const project = await projectService.getProject(Number(projectId));
      projectDetails.value = {
        id: project.id,
        name: project.name,
        description: project.description,
        status: project.status,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        ticket_count: project.ticketCount || 0,
      };
    } catch (err) {
      console.error('Error fetching project details:', err);
    }
  }

  return {
    showLinkedTicketModal,
    showProjectModal,
    projectDetails,
    linkTicket,
    unlinkTicket,
    addToProject,
    removeFromProject,
    fetchProjectDetails,
  };
}
