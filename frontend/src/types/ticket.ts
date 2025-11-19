import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions'
import type { Device } from './device'

// Re-export Device type for convenience
export type { Device }

export interface Ticket {
  id: number;
  title: string;
  status: TicketStatus;
  priority: TicketPriority;
  created: string;
  modified: string;
  assignee: string | null;
  requester: string;
  devices?: Device[];
  linkedTickets: number[];
  project?: string;
  projects?: any; // Backend may return projects array
  notesAndComments?: {
    id: number;
    content: string;
    author: string;
    createdAt: string;
    attachments?: { url: string; name: string }[];
  }[];
  articleContent?: string;
}
