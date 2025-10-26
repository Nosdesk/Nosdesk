import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions'
import type { Device } from './device'

export interface Ticket {
  id: number;
  title: string;
  status: TicketStatus;
  priority: TicketPriority;
  created: string;
  modified: string;
  assignee: string;
  requester: string;
  devices?: Device[];
  linkedTickets: number[];
  project?: string;
  notesAndComments?: {
    id: number;
    content: string;
    author: string;
    createdAt: string;
    attachments?: { url: string; name: string }[];
  }[];
  articleContent?: string;
}
