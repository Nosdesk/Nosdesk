<!-- TicketView.vue -->
<script setup lang="ts">
import { ref, watch, computed, onMounted, onUnmounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useRecentTicketsStore } from "@/stores/recentTickets";
import { useTitleManager } from '@/composables/useTitleManager';
import { useAuthStore } from '@/stores/auth';
import { useSSE } from '@/services/sseService';
import CollaborativeTicketArticle from '@/components/ticketComponents/CollaborativeTicketArticle.vue';
import TicketDetails from '@/components/ticketComponents/TicketDetails.vue'
import DeviceDetails from '@/components/ticketComponents/DeviceDetails.vue';
import DeviceSelectionModal from '@/components/ticketComponents/DeviceSelectionModal.vue';
import CommentsAndAttachments from "@/components/ticketComponents/CommentsAndAttachments.vue";
import LinkedTicketModal from "@/components/ticketComponents/LinkedTicketModal.vue";
import LinkedTicketPreview from "@/components/ticketComponents/LinkedTicketPreview.vue";
import ProjectSelectionModal from "@/components/ticketComponents/ProjectSelectionModal.vue";
import ProjectInfo from "@/components/ticketComponents/ProjectInfo.vue";
import UserAvatar from "@/components/UserAvatar.vue";
import { STATUS_OPTIONS, PRIORITY_OPTIONS } from '@/constants/ticketOptions';
import type { TicketStatus, TicketPriority } from '@/constants/ticketOptions';
import ticketService from '@/services/ticketService';
import userService from '@/services/userService';
import type { Ticket, Device as TicketDevice, Attachment, Project, Comment } from '@/services/ticketService';
import type { User } from '@/services/userService';
import BackButton from '@/components/common/BackButton.vue';
import DeleteButton from '@/components/common/DeleteButton.vue';
import { projectService } from '@/services/projectService';
import * as deviceService from '@/services/deviceService';
import type { Device } from '@/types/device';
import apiClient from '@/services/apiConfig';

// User info interface for comments
interface UserInfo {
  uuid: string;
  name: string;
}

// Custom attachment type for compatibility with existing components
interface UIAttachment {
  url: string;
  name: string;
  id: number;
  comment_id: number;
}

// Interface for comments with UI-compatible attachments
interface CommentWithAttachments {
  id: number;
  content: string;
  user_uuid: string;
  createdAt: string;
  created_at: string;
  ticket_id: number;
  attachments?: UIAttachment[];
  user?: UserInfo;
}

// Extended ticket interface with additional UI-specific properties
interface LocalTicket extends Ticket {
  commentsAndAttachments?: CommentWithAttachments[];
  project?: string;
  devices?: TicketDevice[];
}

const route = useRoute();
const router = useRouter();
const ticket = ref<LocalTicket | null>(null);
const projectDetails = ref<Project | null>(null);
const recentTicketsStore = useRecentTicketsStore();
const titleManager = useTitleManager();
const authStore = useAuthStore();

// Setup SSE
const { addEventListener, removeEventListener, isConnected, connect, disconnect, triggerReconnection } = useSSE();

// Remove redundant SSE reconnection logic since it's now handled by the service
// The service handles reconnection automatically with exponential backoff

// Watch SSE connection status for UI updates only
watch(isConnected, (connected) => {
  if (connected) {
    console.log('SSE: Connection established');
  } else {
    console.log('SSE: Connection lost - service will handle reconnection automatically');
  }
});

// Add users state
const users = ref<User[]>([]);
const loadingUsers = ref(true);
const usersError = ref<string | null>(null);

const selectedStatus = ref<TicketStatus>("open");
const selectedPriority = ref<TicketPriority>("low");
const showDeviceModal = ref(false);

// Add a watcher to debug the requester value when modal opens
watch(showDeviceModal, (isOpen) => {
  if (isOpen && import.meta.env.DEV) {
    console.log('TicketView: DeviceModal opened, ticket data:', {
      ticketId: ticket.value?.id,
      requester: ticket.value?.requester,
      requesterUser: ticket.value?.requester_user,
      fullTicket: ticket.value
    });
  }
});
const showProjectModal = ref(false);
const loading = ref(true);
const error = ref<string | null>(null);

// Add isNewTicket flag and editingTitle state
const isNewTicket = ref(false);
const editingTitle = ref(false);
const titleInput = ref<HTMLInputElement | null>(null);
const newTitleValue = ref('');

// Add a function to transform backend Device to frontend Device format
const transformDevice = (backendDevice: Device): TicketDevice => {
  return {
    id: backendDevice.id,
    name: backendDevice.name,
    hostname: backendDevice.hostname,
    serial_number: backendDevice.serial_number,
    model: backendDevice.model,
    warranty_status: backendDevice.warranty_status
  };
};

// Remove the fetchUsers function - we'll use the lookup service instead

const fetchTicket = async (ticketId: string | string[]) => {
  const id = Number(ticketId);
  loading.value = true;
  error.value = null;
  
  // Only log in development mode
  if (import.meta.env.DEV) {
    console.log(`Fetching ticket #${id} from server...`);
  }
  
  try {
    const fetchedTicket = await ticketService.getTicketById(id);
    
    if (!fetchedTicket) {
      router.push("/404");
      return;
    }

    // Only log detailed data in development mode
    if (import.meta.env.DEV) {
      console.log(`Received ticket data from server:`, fetchedTicket);
      console.log(`Devices from server:`, fetchedTicket.devices);
      console.log(`Linked tickets from server:`, fetchedTicket.linked_tickets);
      console.log(`Projects from server:`, fetchedTicket.projects);
    }

    // Check if this is a new ticket (title is "New Ticket")
    isNewTicket.value = fetchedTicket.title === 'New Ticket';
    
    // If it's a new ticket, set the title value for editing
    if (isNewTicket.value) {
      editingTitle.value = true;
      newTitleValue.value = '';
      // Set a slight delay to ensure the DOM is ready
      setTimeout(() => {
        if (titleInput.value) {
          titleInput.value.focus();
        }
      }, 100);
    }

    // Transform comments to commentsAndAttachments format expected by components
    const commentsAndAttachments = fetchedTicket.comments?.map(comment => ({
      id: comment.id,
      content: comment.content,
      user_uuid: comment.user_uuid,
      createdAt: comment.created_at,
      created_at: comment.created_at,
      ticket_id: comment.ticket_id,
      attachments: comment.attachments?.map(att => ({
        id: att.id,
        url: att.url,
        name: att.name,
        comment_id: att.comment_id
      })) || [],
      user: comment.user // Include the user information from the backend
    })) || [];

    // Sort comments by created_at date in descending order (newest first)
    commentsAndAttachments.sort((a, b) => {
      return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
    });

    // Transform devices to match the frontend Device type
    let transformedDevices: TicketDevice[] = [];
    
    // Handle devices from backend (now supports multiple devices)
    if (fetchedTicket.devices && fetchedTicket.devices.length > 0) {
      transformedDevices = fetchedTicket.devices.map(device => ({
        id: device.id,
        name: device.name,
        hostname: device.hostname,
        serial_number: device.serial_number,
        model: device.model,
        warranty_status: device.warranty_status
      }));
    }
    
    if (import.meta.env.DEV) {
      console.log('Transformed devices:', transformedDevices);
    }

    // Update the ticket with the fetched data
    ticket.value = {
      ...fetchedTicket,
      linkedTickets: fetchedTicket.linked_tickets || fetchedTicket.linkedTickets || [],
      devices: transformedDevices,
      commentsAndAttachments
    } as unknown as LocalTicket;

    if (import.meta.env.DEV) {
      console.log(`Processed linked tickets:`, ticket.value.linkedTickets);
    }

    // Update the selected values to match the ticket
    selectedStatus.value = ticket.value.status;
    selectedPriority.value = ticket.value.priority;
    
    // Set the ticket in the title manager for proper SSE integration
    titleManager.setTicket({
      id: ticket.value.id,
      title: ticket.value.title
    });

    const fromRecent = route.query.fromRecent === "true";
    
    // Check if the ticket already exists in the recent tickets store
    const existingTicket = recentTicketsStore.recentTickets.find(t => t.id === id);
    
    if (existingTicket) {
      // Only log in development mode
      if (import.meta.env.DEV) {
        console.log(`Ticket #${id} already exists in recent tickets store with title: "${existingTicket.title}"`);
        console.log(`Server returned title: "${fetchedTicket.title}"`);
      }
      
      // Only update specific fields, preserving the title if it was manually changed
      recentTicketsStore.updateTicketData(id, {
        status: fetchedTicket.status,
        requester: fetchedTicket.requester,
        assignee: fetchedTicket.assignee,
        modified: fetchedTicket.modified
      });
    } else {
      // If the ticket doesn't exist in the store, add it
      if (import.meta.env.DEV) {
        console.log(`Adding ticket #${id} to recent tickets store with title: "${fetchedTicket.title}"`);
      }
      recentTicketsStore.addRecentTicket(
        {
          id: ticket.value.id,
          title: ticket.value.title,
          status: ticket.value.status,
          requester: ticket.value.requester,
          assignee: ticket.value.assignee,
          created: ticket.value.created,
          modified: ticket.value.modified
        },
        fromRecent,
      );
    }

    // Update project details if projects are available
    if (fetchedTicket.projects && fetchedTicket.projects.length > 0) {
      // Use the project data directly from the API response
      projectDetails.value = fetchedTicket.projects[0];
      // Store the project ID in the ticket for reference
      ticket.value.project = String(fetchedTicket.projects[0].id);
    } else if (ticket.value.project) {
      await fetchProjectDetails(ticket.value.project);
    }
    
    if (import.meta.env.DEV) {
      console.log('Ticket data refreshed:', ticket.value);
    }
  } catch (err) {
    console.error(`Error fetching ticket ${id}:`, err);
    error.value = 'Failed to load ticket. Please try again later.';
  } finally {
    loading.value = false;
  }
};

// Add a refresh function to reload the ticket data
const refreshTicket = async () => {
  if (ticket.value) {
    const ticketId = ticket.value.id;
    
    // Only log in development mode
    if (import.meta.env.DEV) {
      console.log(`Refreshing ticket #${ticketId} data...`);
      
      // Store the current title and linked tickets before refreshing
      const currentTitle = ticket.value.title;
      const currentLinkedTickets = [...(ticket.value.linkedTickets || [])];
      console.log(`Current title before refresh: "${currentTitle}"`);
      console.log(`Current linked tickets before refresh: ${JSON.stringify(currentLinkedTickets)}`);
    }
    
    // Fetch fresh data from the server
    await fetchTicket(String(ticketId));
    
    if (import.meta.env.DEV) {
      // Check if the title was preserved
      console.log(`Title after refresh: "${ticket.value?.title}"`);
      console.log(`Linked tickets after refresh: ${JSON.stringify(ticket.value?.linkedTickets)}`);
    }
    
    // Ensure the recent tickets store has the latest title
    if (ticket.value && import.meta.env.DEV) {
      const currentTitle = ticket.value.title;
      console.log(`Title changed during refresh. Updating recent tickets store with new title: "${ticket.value.title}"`);
      recentTicketsStore.updateTicketData(ticketId, {
        title: ticket.value.title
      });
    }
  }
};

// Format users for the UserSelection component - now empty since we use lookup service
const formattedUsers = computed(() => {
  return []; // UserSelection component will be updated to use lookup service
});

const formattedDate = (dateString: string | undefined) => {
  if (!dateString) return "";
  const date = new Date(dateString);
  return date.toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
};

const formattedCreatedDate = computed(() => formattedDate(ticket.value?.created));
const formattedModifiedDate = computed(() => formattedDate(ticket.value?.modified));

// Fetch ticket data when component mounts
onMounted(async () => {
  if (route.params.id) {
    fetchTicket(route.params.id);
  }
  
  // Set up SSE event listeners
  console.log('🔧 TicketView: Setting up SSE event listeners...');
  addEventListener('ticket-updated', handleTicketUpdated);
  addEventListener('comment-added', handleCommentAdded);
  addEventListener('comment-deleted', handleCommentDeleted);
  addEventListener('device-linked', handleDeviceLinked);
  addEventListener('device-unlinked', handleDeviceUnlinked);
  addEventListener('device-updated', handleDeviceUpdated);
  addEventListener('ticket-linked', handleTicketLinked);
  addEventListener('ticket-unlinked', handleTicketUnlinked);
  addEventListener('project-assigned', handleProjectAssigned);
  addEventListener('project-unassigned', handleProjectUnassigned);
  console.log('🔧 TicketView: SSE event listeners registered successfully');
  
  // Connect to SSE for real-time updates
  if (authStore.token) {
    connect(route.params.id ? Number(route.params.id) : undefined);
  }
});

// Clean up SSE when component unmounts
onUnmounted(() => {
  removeEventListener('ticket-updated', handleTicketUpdated);
  removeEventListener('comment-added', handleCommentAdded);
  removeEventListener('comment-deleted', handleCommentDeleted);
  removeEventListener('device-linked', handleDeviceLinked);
  removeEventListener('device-unlinked', handleDeviceUnlinked);
  removeEventListener('device-updated', handleDeviceUpdated);
  removeEventListener('ticket-linked', handleTicketLinked);
  removeEventListener('ticket-unlinked', handleTicketUnlinked);
  removeEventListener('project-assigned', handleProjectAssigned);
  removeEventListener('project-unassigned', handleProjectUnassigned);
  disconnect();
});

watch(
  () => route.params.id,
  (newId) => {
    if (newId) {
      fetchTicket(newId);
    }
  }
);

// Helper function to get current UTC datetime formatted for the backend
const getCurrentUTCDateTime = () => {
  const now = new Date();
  return now.toISOString(); // Return full ISO datetime string
};

const updateStatus = async (newStatus: string) => {
  if (!ticket.value) return;
  
  const typedStatus = newStatus as TicketStatus;
  if (ticket.value.status === typedStatus) return; // No change needed
  
  try {
    console.log(`Updating status from ${ticket.value.status} to ${typedStatus}`);
    
    // Get current UTC datetime
    const nowDateTime = getCurrentUTCDateTime();
    
    // Send the update to the backend first
    const updateData = { 
      status: typedStatus,
      modified: nowDateTime
    };
    
    const updatedTicket = await ticketService.updateTicket(ticket.value.id, updateData);
    console.log(`Status updated successfully:`, updatedTicket);
    
    // Update local state with the response from server
    ticket.value.status = updatedTicket.status;
    ticket.value.modified = updatedTicket.modified;
    selectedStatus.value = updatedTicket.status;
    
    // Update the ticket in the recent tickets store
    recentTicketsStore.updateTicketData(ticket.value.id, {
      status: updatedTicket.status,
      modified: updatedTicket.modified
    });
    
  } catch (err) {
    console.error(`Error updating ticket status:`, err);
    // Show error notification if needed
  }
};

const updatePriority = async (newPriority: string) => {
  if (!ticket.value) return;
  
  const typedPriority = newPriority as TicketPriority;
  if (ticket.value.priority === typedPriority) return; // No change needed
  
  try {
    console.log(`Updating priority from ${ticket.value.priority} to ${typedPriority}`);
    
    // Get current UTC datetime
    const nowDateTime = getCurrentUTCDateTime();
    
    // Send the update to the backend first
    const updateData = { 
      priority: typedPriority,
      modified: nowDateTime
    };
    
    const updatedTicket = await ticketService.updateTicket(ticket.value.id, updateData);
    console.log(`Priority updated successfully:`, updatedTicket);
    
    // Update local state with the response from server
    ticket.value.priority = updatedTicket.priority;
    ticket.value.modified = updatedTicket.modified;
    selectedPriority.value = updatedTicket.priority;
    
    // Update the ticket in the recent tickets store
    recentTicketsStore.updateTicketData(ticket.value.id, {
      // Priority is not stored in the recent tickets, but we'll update modified date
      modified: updatedTicket.modified
    });
    
  } catch (err) {
    console.error(`Error updating ticket priority:`, err);
    // Show error notification if needed
  }
};

// Add handlers for requester and assignee updates
const updateRequester = async (newRequester: string) => {
  if (!ticket.value) return;
  
  const oldRequester = ticket.value.requester;
  if (oldRequester === newRequester) return; // No change needed
  
  try {
    console.log(`Updating requester from ${oldRequester} to ${newRequester}`);
    
    // Get current UTC datetime
    const nowDateTime = getCurrentUTCDateTime();
    
    // Send the update to the backend first
    const updateData = { 
      requester: newRequester,
      modified: nowDateTime
    };
    console.log('Sending update to backend:', updateData);
    
    const updatedTicket = await ticketService.updateTicket(ticket.value.id, updateData);
    console.log(`Requester updated successfully:`, updatedTicket);
    
    // Update local state with the response from server
    ticket.value.requester = updatedTicket.requester;
    ticket.value.requester_user = updatedTicket.requester_user;
    ticket.value.modified = updatedTicket.modified;
    
    // Update the ticket in the recent tickets store
    recentTicketsStore.updateTicketData(ticket.value.id, {
      requester: updatedTicket.requester,
      modified: updatedTicket.modified
    });
    
  } catch (err) {
    console.error(`Error updating ticket requester:`, err);
    // Show error notification if needed
  }
};

const updateAssignee = async (newAssignee: string) => {
  if (!ticket.value) return;
  
  const oldAssignee = ticket.value.assignee;
  if (oldAssignee === newAssignee) return; // No change needed
  
  try {
    console.log(`Updating assignee from ${oldAssignee} to ${newAssignee}`);
    
    // Get current UTC datetime
    const nowDateTime = getCurrentUTCDateTime();
    
    // Send the update to the backend first
    const updateData = { 
      assignee: newAssignee,
      modified: nowDateTime
    };
    console.log('Sending update to backend:', updateData);
    
    const updatedTicket = await ticketService.updateTicket(ticket.value.id, updateData);
    console.log(`Assignee updated successfully:`, updatedTicket);
    
    // Update local state with the response from server
    ticket.value.assignee = updatedTicket.assignee;
    ticket.value.assignee_user = updatedTicket.assignee_user;
    ticket.value.modified = updatedTicket.modified;
    
    // Update the ticket in the recent tickets store
    recentTicketsStore.updateTicketData(ticket.value.id, {
      assignee: updatedTicket.assignee,
      modified: updatedTicket.modified
    });
    
  } catch (err) {
    console.error(`Error updating ticket assignee:`, err);
    // Show error notification if needed
  }
};

// Add device field update handler
const updateDeviceField = async (deviceId: number, field: string, newValue: string) => {
  if (!ticket.value || !ticket.value.devices) return;
  
  // Find the device in the ticket's device list
  const deviceIndex = ticket.value.devices.findIndex(d => d.id === deviceId);
  if (deviceIndex === -1) {
    console.error(`Device ${deviceId} not found in ticket devices`);
    return;
  }
  
  const device = ticket.value.devices[deviceIndex];
  const oldValue = (device as any)[field];
  
  if (oldValue === newValue) return; // No change needed
  
  try {
    console.log(`🔧 TicketView: Updating device ${deviceId} field ${field} from "${oldValue}" to "${newValue}" (Local Update)`);
    
    // Optimistically update the local state for immediate UI feedback
    const updatedDevices = [...ticket.value.devices];
    (updatedDevices[deviceIndex] as any)[field] = newValue;
    
    // Update the ticket state immutably
    ticket.value = {
      ...ticket.value,
      devices: updatedDevices
    };
    
    // Send the update to the backend
    const updateData = {
      [field]: newValue
    };
    
    const updatedDevice = await deviceService.updateDevice(deviceId, updateData);
    console.log(`🔧 TicketView: Device field updated successfully via API:`, updatedDevice);
    
    // The SSE event will also update this, but we've already updated locally for responsiveness
    console.log(`🔧 TicketView: Local state updated for device ${deviceId}`);
    
  } catch (err) {
    console.error(`🔧 TicketView: Error updating device field:`, err);
    
    // Revert the optimistic update on error
    if (ticket.value.devices) {
      const revertedDevices = [...ticket.value.devices];
      (revertedDevices[deviceIndex] as any)[field] = oldValue;
      
      ticket.value = {
        ...ticket.value,
        devices: revertedDevices
      };
      
      console.log(`🔧 TicketView: Reverted local state for device ${deviceId} due to API error`);
    }
    
    // Show error notification if needed
    // Optionally revert the UI state here if needed
  }
};

const emit = defineEmits<{
  (e: 'update:ticket', ticket: { id: number; title: string } | null): void;
}>();

// Simple watcher to emit ticket updates when ticket loads or changes
watch(ticket, (newTicket) => {
  if (newTicket) {
    emit('update:ticket', {
      id: newTicket.id,
      title: newTicket.title
    });
  } else {
    emit('update:ticket', null);
  }
}, { immediate: true });

const updateTicketTitle = async (newTitle: string) => {
  if (ticket.value) {
    const oldTitle = ticket.value.title;
    const ticketId = ticket.value.id;
    
    console.log(`Updating ticket #${ticketId} title from "${oldTitle}" to "${newTitle}"`);
    
    try {
      // Update the local state
      ticket.value.title = newTitle;
      
      // Get current UTC datetime
      const nowDateTime = getCurrentUTCDateTime();
      ticket.value.modified = nowDateTime;
      
      // Update the ticket in the recent tickets store BEFORE the API call
      console.log('Updating title in recent tickets store...');
      recentTicketsStore.updateTicketData(ticketId, {
        title: newTitle,
        modified: nowDateTime
      });
      
      // Send the update to the backend
      console.log('Sending title update to backend...');
      await ticketService.updateTicket(ticketId, { 
        title: newTitle,
        modified: nowDateTime
      });
      
      console.log('Title update successful');
      
      // Refresh the ticket data to ensure it's up to date
      await refreshTicket();
    } catch (err) {
      console.error(`Error updating ticket title:`, err);
      // Revert UI if update fails
      ticket.value.title = oldTitle;
      
      // Also revert the title in the recent tickets store
      recentTicketsStore.updateTicketData(ticketId, {
        title: oldTitle
      });
    }
  }
};

const handleAddDevice = async (device: Device) => {
  if (ticket.value) {
    try {
      console.log(`Adding device ${device.id} to ticket ${ticket.value.id}`);
      
      // Use the new ticket service to add device to ticket
      await ticketService.addDeviceToTicket(ticket.value.id, device.id);
      console.log(`Device ${device.id} added to ticket ${ticket.value.id}`);
      
      // Optimistically add the device to local state for immediate UI feedback
      const transformedDevice = transformDevice(device);
      if (ticket.value.devices) {
        ticket.value = {
          ...ticket.value,
          devices: [...ticket.value.devices, transformedDevice]
        };
      } else {
        ticket.value = {
          ...ticket.value,
          devices: [transformedDevice]
        };
      }
      console.log(`✅ TicketView: Device ${device.id} added to local state`);
      
      // Close the modal
      showDeviceModal.value = false;
      
      console.log(`Successfully added device ${device.id} to ticket ${ticket.value.id}`);
    } catch (err) {
      console.error(`Error adding device to ticket:`, err);
      // If there's an error, refresh the ticket data to ensure consistency
      await refreshTicket();
    }
  }
};

const removeDevice = async (deviceId: number) => {
  if (ticket.value) {
    try {
      console.log(`Removing device ${deviceId} from ticket ${ticket.value.id}`);
      
      // Use the new ticket service to remove device from ticket
      await ticketService.removeDeviceFromTicket(ticket.value.id, deviceId);
      console.log(`Device ${deviceId} removed from ticket ${ticket.value.id}`);
      
      // Optimistically remove the device from local state for immediate UI feedback
      if (ticket.value.devices) {
        const updatedDevices = ticket.value.devices.filter(d => d.id !== deviceId);
        ticket.value = {
          ...ticket.value,
          devices: updatedDevices
        };
        console.log(`✅ TicketView: Device ${deviceId} removed from local state`);
      }
      
      console.log(`Successfully removed device ${deviceId} from ticket ${ticket.value.id}`);
    } catch (err) {
      console.error(`Error removing device from ticket:`, err);
      // If there's an error, refresh the ticket data to ensure consistency
      await refreshTicket();
    }
  }
};

const showLinkedTicketModal = ref(false);

const handleLinkTicket = async (linkedTicketId: number) => {
  if (ticket.value) {
    if (!ticket.value.linkedTickets) {
      ticket.value.linkedTickets = [];
    }
    
    if (!ticket.value.linkedTickets.includes(linkedTicketId)) {
      try {
        console.log(`Attempting to link ticket ${ticket.value.id} to ticket ${linkedTicketId}`);
        
        // Call the API to link the tickets
        await ticketService.linkTicket(ticket.value.id, linkedTicketId);
        console.log(`API call successful for linking ticket ${ticket.value.id} to ${linkedTicketId}`);
        
        // Don't update the local state here, instead refresh the ticket data
        // to get the updated linked_tickets from the server
        await refreshTicket();
        
        console.log(`Successfully linked ticket ${ticket.value.id} to ticket ${linkedTicketId}`);
        console.log(`Updated linked tickets: ${JSON.stringify(ticket.value.linkedTickets)}`);
      } catch (err) {
        console.error(`Error linking ticket:`, err);
      }
    } else {
      console.log(`Ticket ${linkedTicketId} is already linked to ticket ${ticket.value.id}`);
    }
  }
};

const unlinkTicket = async (linkedTicketId: number) => {
  if (ticket.value) {
    try {
      console.log(`Attempting to unlink ticket ${ticket.value.id} from ticket ${linkedTicketId}`);
      
      // Call the API to unlink the tickets
      await ticketService.unlinkTicket(ticket.value.id, linkedTicketId);
      console.log(`API call successful for unlinking ticket ${ticket.value.id} from ${linkedTicketId}`);
      
      // Don't update local state here, instead refresh the ticket data
      // to get the updated linked_tickets from the server
      await refreshTicket();
      
      console.log(`Successfully unlinked ticket ${ticket.value.id} from ticket ${linkedTicketId}`);
      console.log(`Updated linked tickets: ${JSON.stringify(ticket.value.linkedTickets)}`);
    } catch (err) {
      console.error(`Error unlinking ticket:`, err);
    }
  }
};

const handleAddToProject = async (projectId: number) => {
  if (ticket.value) {
    try {
      // Add the ticket to the project
      await projectService.addTicketToProject(projectId, ticket.value.id);
      
      // Fetch the project details
      const project = await projectService.getProject(projectId);
      
      // Convert the project to the format expected by the API
      projectDetails.value = {
        id: project.id,
        name: project.name,
        description: project.description,
        status: project.status,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        ticket_count: project.ticketCount || 0 // Include ticket count if available
      };
      
      ticket.value.project = String(projectId);
      showProjectModal.value = false;
      
      console.log(`Added ticket ${ticket.value.id} to project ${projectId}`);
      
      // Refresh the ticket to get the updated project information
      await refreshTicket();
    } catch (err) {
      console.error(`Error adding ticket to project:`, err);
      // You could show an error notification here
    }
  }
}

// Update fetchProjectDetails function
const fetchProjectDetails = async (projectId: string) => {
  try {
    const project = await projectService.getProject(Number(projectId));
    
    // Convert the project to the format expected by the API
    projectDetails.value = {
      id: project.id,
      name: project.name,
      description: project.description,
      status: project.status,
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
      ticket_count: project.ticketCount || 0 // Include ticket count if available
    };
  } catch (err) {
    console.error(`Error fetching project details:`, err);
    // You could show an error notification here
  }
}

// Add this function to handle removing a ticket from a project
const removeFromProject = async () => {
  if (ticket.value && ticket.value.project) {
    try {
      const projectId = Number(ticket.value.project);
      
      await projectService.removeTicketFromProject(projectId, ticket.value.id);
      
      ticket.value.project = undefined;
      projectDetails.value = null;
      
      console.log(`Removed ticket ${ticket.value.id} from project ${projectId}`);
    } catch (err) {
      console.error(`Error removing ticket from project:`, err);
      // You could show an error notification here
    }
  }
}

// Add this function to handle project navigation
const viewProject = (projectId: string) => {
  router.push(`/projects/${projectId}`)
}

const getStatusColor = (status: string) => {
  switch (status) {
    case 'active':
      return 'text-green-400'
    case 'completed':
      return 'text-blue-400'
    case 'archived':
      return 'text-gray-400'
    default:
      return 'text-slate-400'
  }
}

// Add function to handle new comments
const handleAddComment = async (data: { content: string; user_uuid: string; files: File[] }) => {
  if (!ticket.value) return;
  
  try {
    // Validate that we have either content or files
    if (!data.content.trim() && (!data.files || data.files.length === 0)) {
      console.log('Comment must have either content or files');
      return;
    }
    
    // If content is empty but we have files, set a placeholder content
    if (!data.content.trim() && data.files && data.files.length > 0) {
      data.content = "Attachment added";
    }
    
    console.log("Handling comment with files:", data);
    
    // Upload files first if any
    let attachments: { id: number; url: string; name: string }[] = [];
    if (data.files && data.files.length > 0) {
      console.log("Uploading files...");
      
      // Create FormData for file upload
      const formData = new FormData();
      data.files.forEach((file) => {
        formData.append("files", file, file.name);
      });
      
      // Use apiClient which automatically handles authentication
      const response = await apiClient.post("/upload", formData, {
        headers: {
          'Content-Type': 'multipart/form-data'
        }
      });
      
      const uploadedFiles = response.data; // [{ id, url, name }]
      attachments = uploadedFiles.map((file: any) => ({
        id: file.id,
        url: file.url,
        name: file.name
      }));
      
      console.log("Files uploaded successfully:", attachments);
    }
    
    // Create the comment with attachments (user info extracted from JWT token)
    const newComment = await ticketService.addCommentToTicket(
      ticket.value.id,
      data.content,
      attachments
    );
    
    console.log('Comment created successfully, response:', newComment);
    
    // Optimistically add the comment to local state for immediate UI feedback
    if (ticket.value.commentsAndAttachments) {
      const optimisticComment: CommentWithAttachments = {
        id: newComment.id,
        content: newComment.content,
        user_uuid: newComment.user_uuid,
        createdAt: newComment.created_at,
        created_at: newComment.created_at,
        ticket_id: newComment.ticket_id,
        attachments: newComment.attachments || [],
        user: newComment.user
      };
      
      // Add to the beginning of the array (newest first) - use immutable update
      ticket.value.commentsAndAttachments = [optimisticComment, ...ticket.value.commentsAndAttachments];
      
      // Mark comment as recently added for visual highlighting
      recentlyAddedCommentIds.value.add(newComment.id);
      setTimeout(() => {
        recentlyAddedCommentIds.value.delete(newComment.id);
      }, 3000); // Highlight for 3 seconds
      
      // Show a brief visual indicator
      showDeviceUpdateIndicator.value = true;
      setTimeout(() => {
        showDeviceUpdateIndicator.value = false;
      }, 2000);
    }
    
  } catch (error) {
    console.error("Error adding comment:", error);
    // If there's an error, refresh the ticket data to ensure consistency
    await refreshTicket();
  }
};

// Add function to handle deleting attachments
const handleDeleteAttachment = async (data: { commentId: number; attachmentIndex: number }) => {
  if (!ticket.value) return;
  
  try {
    console.log('Deleting attachment:', data);
    
    // Get the comment and attachment
    const comment = ticket.value.commentsAndAttachments?.find(c => c.id === data.commentId);
    if (!comment || !comment.attachments || comment.attachments.length <= data.attachmentIndex) {
      console.error('Attachment not found');
      return;
    }
    
    const attachment = comment.attachments[data.attachmentIndex];
    console.log('Attachment details:', attachment);
    
    // Check if the attachment has an ID (from the backend)
    if (attachment.id) {
      // Optimistically remove the attachment from local state for immediate UI feedback
      if (ticket.value.commentsAndAttachments) {
        const commentIndex = ticket.value.commentsAndAttachments.findIndex(c => c.id === data.commentId);
        if (commentIndex !== -1) {
          // Create a new array with the attachment removed
          const updatedAttachments = [...comment.attachments];
          updatedAttachments.splice(data.attachmentIndex, 1);
          
          // Create a new comment with updated attachments
          const updatedComment = { ...comment, attachments: updatedAttachments };
          
          // Create a new comments array with the updated comment
          const updatedComments = [...ticket.value.commentsAndAttachments];
          updatedComments[commentIndex] = updatedComment;
          
          // Update the ticket state immutably
          ticket.value = { ...ticket.value, commentsAndAttachments: updatedComments };
        }
      }
      
      // Delete the attachment from the backend
      await ticketService.deleteAttachment(attachment.id);
      
      console.log('Attachment deleted successfully');
      
      // Check if this is the last attachment and the comment is empty
      if (comment.attachments.length === 1 && (!comment.content || comment.content.trim() === '')) {
        console.log('Last attachment deleted from empty comment. Deleting comment:', comment.id);
        await handleDeleteComment(comment.id);
      }
    } else {
      console.error('Cannot delete attachment without ID. Attachment:', attachment);
    }
  } catch (err) {
    console.error('Error deleting attachment:', err);
    // If there's an error, refresh the ticket data to ensure consistency
    await refreshTicket();
  }
};

// Add function to handle deleting comments
const handleDeleteComment = async (commentId: number) => {
  if (!ticket.value) return;
  
  try {
    console.log('Deleting comment:', commentId);
    
    // Optimistically remove the comment from local state for immediate UI feedback
    if (ticket.value.commentsAndAttachments) {
      // Create a new array without the deleted comment
      const updatedComments = ticket.value.commentsAndAttachments.filter(c => c.id !== commentId);
      ticket.value = { ...ticket.value, commentsAndAttachments: updatedComments };
    }
    
    // Delete the comment from the backend
    await ticketService.deleteComment(commentId);
    
    console.log('Comment deleted successfully');
    
    // Show a brief visual indicator
    showDeviceUpdateIndicator.value = true;
    setTimeout(() => {
      showDeviceUpdateIndicator.value = false;
    }, 2000);
    
  } catch (err) {
    console.error('Error deleting comment:', err);
    // If there's an error, refresh the ticket data to ensure consistency
    await refreshTicket();
  }
};

// Add function to handle ticket deletion
const deleteTicket = async () => {
  if (!ticket.value) return;
  
  const ticketId = ticket.value.id;
  console.log(`Deleting ticket #${ticketId}...`);
  
  try {
    // Call the API to delete the ticket
    await ticketService.deleteTicket(ticketId);
    
    // Remove the ticket from the recent tickets store
    recentTicketsStore.removeRecentTicket(ticketId);
    
    console.log(`Ticket #${ticketId} deleted successfully`);
    
    // Navigate back to the tickets list
    router.push('/tickets');
  } catch (err) {
    console.error(`Error deleting ticket #${ticketId}:`, err);
    // You could show an error notification here
  }
};

const navigateToDeviceView = (deviceId: number) => {
  router.push({
    path: `/devices/${deviceId}`,
    query: { fromTicket: String(ticket.value?.id) }
  });
};

// SSE Event Handlers
const handleTicketUpdated = (eventData: any) => {
  console.log('SSE: Raw event data:', eventData);
  
  // The event data structure is { type: "TicketUpdated", data: { ticket_id, field, value, ... } }
  const data = eventData.data || eventData;
  
  console.log('SSE: Received event for ticket', data.ticket_id, 'current ticket:', ticket.value?.id);
  if (!ticket.value || data.ticket_id !== ticket.value.id) return;
  
  console.log('SSE: Update by user', data.updated_by, 'current user:', authStore.user?.uuid);
  console.log('SSE: Processing ticket update:', data);
  
  // Update the specific field that was changed
  if (data.field === 'title') {
    console.log('SSE: Updating title from', ticket.value.title, 'to', data.value);
    ticket.value.title = data.value;
    
    // Update the title manager immediately for HeaderTitle component
    titleManager.setTicket({
      id: ticket.value.id,
      title: data.value
    });
    
    // Directly emit the update to the parent for immediate reactivity
    emit('update:ticket', {
      id: ticket.value.id,
      title: data.value
    });
    console.log('SSE: Title updated, new value:', ticket.value.title);
  } else if (data.field === 'status') {
    ticket.value.status = data.value;
    selectedStatus.value = data.value;
  } else if (data.field === 'priority') {
    ticket.value.priority = data.value;
    selectedPriority.value = data.value;
  } else if (data.field === 'requester') {
    console.log('SSE: Updating requester from', ticket.value.requester, 'to', data.value);
    // Handle both old format (just UUID string) and new format (object with user_info)
    if (typeof data.value === 'string') {
      ticket.value.requester = data.value;
    } else if (data.value && data.value.uuid) {
      ticket.value.requester = data.value.uuid;
      if (data.value.user_info) {
        ticket.value.requester_user = data.value.user_info;
      }
    }
    console.log('SSE: Requester updated, new value:', ticket.value.requester, 'user:', ticket.value.requester_user);
  } else if (data.field === 'assignee') {
    console.log('SSE: Updating assignee from', ticket.value.assignee, 'to', data.value);
    // Handle both old format (just UUID string) and new format (object with user_info)
    if (typeof data.value === 'string') {
      ticket.value.assignee = data.value;
    } else if (data.value && data.value.uuid) {
      ticket.value.assignee = data.value.uuid;
      if (data.value.user_info) {
        ticket.value.assignee_user = data.value.user_info;
      }
    }
    console.log('SSE: Assignee updated, new value:', ticket.value.assignee, 'user:', ticket.value.assignee_user);
  }
  
  // Update the recent tickets store
  recentTicketsStore.updateTicketData(ticket.value.id, {
    title: ticket.value.title,
    status: ticket.value.status,
    requester: ticket.value.requester,
    assignee: ticket.value.assignee,
  });
};

const handleCommentAdded = (data: any) => {
  console.log('💬 TicketView: handleCommentAdded called with data:', data);
  
  // Handle both event formats - the data might be nested or direct
  const eventData = data.data || data;
  console.log('💬 TicketView: Processed event data:', eventData);
  
  if (!ticket.value) {
    console.log('💬 TicketView: No ticket loaded, ignoring comment added event');
    return;
  }
  
  if (eventData.ticket_id !== ticket.value.id) {
    console.log(`💬 TicketView: Event for different ticket (${eventData.ticket_id} vs ${ticket.value.id}), ignoring`);
    return;
  }
  
  console.log('💬 TicketView: Comment added event matches current ticket:', eventData);
  
  // Extract the comment data from the event
  const commentData = eventData.comment;
  if (!commentData) {
    console.log('💬 TicketView: No comment data found in event, ignoring');
    return;
  }
  
  // Check if comment already exists to avoid duplicates
  if (ticket.value.commentsAndAttachments) {
    const existingComment = ticket.value.commentsAndAttachments.find(c => c.id === commentData.id);
    if (existingComment) {
      console.log('💬 TicketView: Comment already exists, ignoring duplicate');
      return;
    }
    
    // Convert the comment to the expected format
    const newComment: CommentWithAttachments = {
      id: commentData.id,
      content: commentData.content,
      user_uuid: commentData.user_uuid || commentData.user_id,
      createdAt: commentData.createdAt || commentData.created_at,
      created_at: commentData.created_at || commentData.createdAt,
      ticket_id: commentData.ticket_id,
      attachments: commentData.attachments || [],
      user: commentData.user
    };
    
    console.log('💬 TicketView: Adding new comment to local state:', newComment);
    
    // Use immutable update to add to the beginning of the array (newest first)
    ticket.value = {
      ...ticket.value,
      commentsAndAttachments: [newComment, ...ticket.value.commentsAndAttachments]
    };
    
    console.log(`✅ TicketView: Added comment from ${newComment.user?.name || newComment.user_uuid} (${ticket.value.commentsAndAttachments?.length} total comments)`);
    
    // Mark comment as recently added for visual highlighting
    recentlyAddedCommentIds.value.add(newComment.id);
    setTimeout(() => {
      recentlyAddedCommentIds.value.delete(newComment.id);
    }, 3000); // Highlight for 3 seconds
    
    // Show a brief visual indicator
    showDeviceUpdateIndicator.value = true;
    setTimeout(() => {
      showDeviceUpdateIndicator.value = false;
    }, 2000);
    
  } else {
    console.log('💬 TicketView: No commentsAndAttachments array, initializing...');
    const newComment: CommentWithAttachments = {
      id: commentData.id,
      content: commentData.content,
      user_uuid: commentData.user_uuid || commentData.user_id,
      createdAt: commentData.createdAt || commentData.created_at,
      created_at: commentData.created_at || commentData.createdAt,
      ticket_id: commentData.ticket_id,
      attachments: commentData.attachments || [],
      user: commentData.user
    };
    
    ticket.value = {
      ...ticket.value,
      commentsAndAttachments: [newComment]
    };
  }
};

const handleCommentDeleted = (data: any) => {
  console.log('💬 TicketView: handleCommentDeleted called with data:', data);
  
  // Handle both event formats - the data might be nested or direct
  const eventData = data.data || data;
  console.log('💬 TicketView: Processed event data:', eventData);
  
  if (!ticket.value) {
    console.log('💬 TicketView: No ticket loaded, ignoring comment deleted event');
    return;
  }
  
  if (eventData.ticket_id !== ticket.value.id) {
    console.log(`💬 TicketView: Event for different ticket (${eventData.ticket_id} vs ${ticket.value.id}), ignoring`);
    return;
  }
  
  console.log('💬 TicketView: Comment deleted event matches current ticket:', eventData);
  
  // Remove the comment from the local state using immutable update
  if (ticket.value.commentsAndAttachments) {
    const originalLength = ticket.value.commentsAndAttachments.length;
    const commentToDelete = ticket.value.commentsAndAttachments.find(c => c.id === eventData.comment_id);
    
    if (commentToDelete) {
      // Create a new array without the deleted comment
      const updatedComments = ticket.value.commentsAndAttachments.filter(
        comment => comment.id !== eventData.comment_id
      );
      
      // Update the ticket state immutably
      ticket.value = {
        ...ticket.value,
        commentsAndAttachments: updatedComments
      };
      
      console.log(`✅ TicketView: Removed comment from ${commentToDelete.user?.name || commentToDelete.user_uuid} (${ticket.value.commentsAndAttachments?.length} comments remaining)`);
      
      // Show a brief visual indicator
      showDeviceUpdateIndicator.value = true;
      setTimeout(() => {
        showDeviceUpdateIndicator.value = false;
      }, 2000);
      
    } else {
      console.log(`💬 TicketView: Comment ${eventData.comment_id} not found in current ticket comments for deletion`);
    }
  } else {
    console.log('💬 TicketView: No commentsAndAttachments array to delete from');
  }
};

const handleDeviceLinked = async (data: any) => {
  console.log('🔗 TicketView: handleDeviceLinked called with data:', data);
  
  // Handle both event formats - the data might be nested or direct
  const eventData = data.data || data;
  console.log('🔗 TicketView: Processed event data:', eventData);
  
  if (!ticket.value) {
    console.log('🔗 TicketView: No ticket loaded, ignoring device linked event');
    return;
  }
  
  if (eventData.ticket_id !== ticket.value.id) {
    console.log(`🔗 TicketView: Event for different ticket (${eventData.ticket_id} vs ${ticket.value.id}), ignoring`);
    return;
  }
  
  console.log('🔗 TicketView: Device linked event matches current ticket:', eventData);
  
  try {
    // Fetch the device information to add to the ticket
    console.log(`🔗 TicketView: Fetching device ${eventData.device_id} details...`);
    const device = await deviceService.getDeviceById(eventData.device_id);
    console.log('🔗 TicketView: Fetched device details:', device);
    
    // Add the device to the ticket's device list if it's not already there
    if (ticket.value.devices) {
      const existingDevice = ticket.value.devices.find(d => d.id === eventData.device_id);
      if (!existingDevice) {
        // Use immutable update to add the device
        ticket.value = {
          ...ticket.value,
          devices: [...ticket.value.devices, device]
        };
        console.log(`✅ TicketView: Added device ${device.hostname || device.name} to ticket (${ticket.value.devices?.length} total devices)`);
      } else {
        console.log(`🔗 TicketView: Device ${device.hostname || device.name} already in ticket`);
      }
    } else {
      // Initialize devices array with the new device
      ticket.value = {
        ...ticket.value,
        devices: [device]
      };
      console.log(`✅ TicketView: Added first device ${device.hostname || device.name} to ticket`);
    }
    
    // Show a brief visual indicator
    showDeviceUpdateIndicator.value = true;
    setTimeout(() => {
      showDeviceUpdateIndicator.value = false;
    }, 2000);
    
  } catch (error) {
    console.error('🔗 TicketView: Error fetching linked device:', error);
    // Fallback to full refresh if device fetch fails
    console.log('🔗 TicketView: Falling back to full ticket refresh...');
    refreshTicket();
  }
};

const handleDeviceUnlinked = (data: any) => {
  console.log('🔗 TicketView: handleDeviceUnlinked called with data:', data);
  
  // Handle both event formats - the data might be nested or direct
  const eventData = data.data || data;
  console.log('🔗 TicketView: Processed event data:', eventData);
  
  if (!ticket.value) {
    console.log('🔗 TicketView: No ticket loaded, ignoring device unlinked event');
    return;
  }
  
  if (eventData.ticket_id !== ticket.value.id) {
    console.log(`🔗 TicketView: Event for different ticket (${eventData.ticket_id} vs ${ticket.value.id}), ignoring`);
    return;
  }
  
  console.log('🔗 TicketView: Device unlinked event matches current ticket:', eventData);
  
  // Remove the device from the ticket's device list using immutable update
  if (ticket.value.devices) {
    const deviceIndex = ticket.value.devices.findIndex(d => d.id === eventData.device_id);
    if (deviceIndex !== -1) {
      const removedDevice = ticket.value.devices[deviceIndex];
      
      // Create a new devices array without the removed device
      const updatedDevices = ticket.value.devices.filter(d => d.id !== eventData.device_id);
      
      // Update the ticket state immutably
      ticket.value = {
        ...ticket.value,
        devices: updatedDevices
      };
      
      console.log(`✅ TicketView: Removed device ${removedDevice.hostname || removedDevice.name} from ticket (${ticket.value.devices?.length} devices remaining)`);
      
      // Show a brief visual indicator
      showDeviceUpdateIndicator.value = true;
      setTimeout(() => {
        showDeviceUpdateIndicator.value = false;
      }, 2000);
      
    } else {
      console.log(`🔗 TicketView: Device ${eventData.device_id} not found in current ticket devices for removal`);
    }
  } else {
    console.log('🔗 TicketView: No devices in ticket to remove');
  }
};

const handleDeviceUpdated = (data: any) => {
  // Enhanced debugging for device updates
  console.log('🔧 SSE: Raw device update event received:', data);
  
  // Handle both event formats
  const eventData = data.data || data;
  
  console.log('🔧 SSE: Processed event data:', {
    device_id: eventData.device_id,
    field: eventData.field,
    value: eventData.value,
    updated_by: eventData.updated_by,
    timestamp: eventData.timestamp
  });
  
  // Check if this device is linked to the current ticket
  if (ticket.value?.devices) {
    const deviceIndex = ticket.value.devices.findIndex(d => d.id === eventData.device_id);
    console.log('🔧 SSE: Device search result:', {
      searched_device_id: eventData.device_id,
      found_at_index: deviceIndex,
      total_devices: ticket.value.devices.length,
      device_ids: ticket.value.devices.map(d => d.id)
    });
    
    if (deviceIndex !== -1) {
      // Update the specific field that was changed using immutable update
      if (eventData.field && eventData.value !== undefined) {
        const oldValue = (ticket.value.devices[deviceIndex] as any)[eventData.field];
        
        // Create a new devices array with the updated device
        const updatedDevices = [...ticket.value.devices];
        (updatedDevices[deviceIndex] as any)[eventData.field] = eventData.value;
        
        // Update the ticket state immutably
        ticket.value = {
          ...ticket.value,
          devices: updatedDevices
        };
        
        console.log(`✅ SSE: Updated device ${eventData.device_id} field ${eventData.field} from "${oldValue}" to "${eventData.value}"`);
        
        // Show a brief visual indicator
        showDeviceUpdateIndicator.value = true;
        setTimeout(() => {
          showDeviceUpdateIndicator.value = false;
        }, 2000);
      } else {
        console.warn('🔧 SSE: Invalid device update data - missing field or value');
      }
    } else {
      console.log('🔧 SSE: Device not found in current ticket devices - ignoring update');
    }
  } else {
    console.log('🔧 SSE: No devices in current ticket - ignoring update');
  }
};

// Add visual indicator for real-time updates
const showDeviceUpdateIndicator = ref(false);
const recentlyAddedCommentIds = ref<Set<number>>(new Set());

// Computed property for comments to ensure proper reactivity
const comments = computed(() => {
  return ticket.value?.commentsAndAttachments || [];
});

// Computed property for devices to ensure proper reactivity
const devices = computed(() => {
  return ticket.value?.devices || [];
});

const handleTicketLinked = async (data: any) => {
  console.log('🎫 TicketView: handleTicketLinked called with data:', data);
  
  // Handle both event formats - the data might be nested or direct
  const eventData = data.data || data;
  console.log('🎫 TicketView: Processed event data:', eventData);
  
  if (!ticket.value) {
    console.log('🎫 TicketView: No ticket loaded, ignoring ticket linked event');
    return;
  }
  
  // Check if this event is for the current ticket (either as source or target)
  const isSourceTicket = eventData.ticket_id === ticket.value.id;
  const isTargetTicket = eventData.linked_ticket_id === ticket.value.id;
  
  if (!isSourceTicket && !isTargetTicket) {
    console.log(`🎫 TicketView: Event for different tickets (${eventData.ticket_id} <-> ${eventData.linked_ticket_id} vs ${ticket.value.id}), ignoring`);
    return;
  }
  
  console.log('🎫 TicketView: Ticket linked event affects current ticket:', eventData);
  
  try {
    // Determine which ticket ID to add to our linked tickets list
    const linkedTicketId = isSourceTicket ? eventData.linked_ticket_id : eventData.ticket_id;
    
    console.log(`🎫 TicketView: Adding linked ticket ${linkedTicketId} to current ticket ${ticket.value.id}`);
    
    // Add the linked ticket to the current ticket's linkedTickets array if it's not already there
    if (ticket.value.linkedTickets) {
      const existingLinkedTicket = ticket.value.linkedTickets.find(id => id === linkedTicketId);
      if (!existingLinkedTicket) {
        ticket.value.linkedTickets.push(linkedTicketId);
        console.log(`✅ TicketView: Added linked ticket ${linkedTicketId} to ticket (${ticket.value.linkedTickets.length} total linked tickets)`);
      } else {
        console.log(`🎫 TicketView: Linked ticket ${linkedTicketId} already in ticket`);
      }
    } else {
      ticket.value.linkedTickets = [linkedTicketId];
      console.log(`✅ TicketView: Added first linked ticket ${linkedTicketId} to ticket`);
    }
    
    // Show a brief visual indicator
    showDeviceUpdateIndicator.value = true;
    setTimeout(() => {
      showDeviceUpdateIndicator.value = false;
    }, 2000);
    
  } catch (error) {
    console.error('🎫 TicketView: Error handling linked ticket:', error);
    // Fallback to full refresh if something fails
    console.log('🎫 TicketView: Falling back to full ticket refresh...');
    refreshTicket();
  }
};

const handleTicketUnlinked = (data: any) => {
  console.log('🎫 TicketView: handleTicketUnlinked called with data:', data);
  
  // Handle both event formats - the data might be nested or direct
  const eventData = data.data || data;
  console.log('🎫 TicketView: Processed event data:', eventData);
  
  if (!ticket.value) {
    console.log('🎫 TicketView: No ticket loaded, ignoring ticket unlinked event');
    return;
  }
  
  // Check if this event is for the current ticket (either as source or target)
  const isSourceTicket = eventData.ticket_id === ticket.value.id;
  const isTargetTicket = eventData.linked_ticket_id === ticket.value.id;
  
  if (!isSourceTicket && !isTargetTicket) {
    console.log(`🎫 TicketView: Event for different tickets (${eventData.ticket_id} <-> ${eventData.linked_ticket_id} vs ${ticket.value.id}), ignoring`);
    return;
  }
  
  console.log('🎫 TicketView: Ticket unlinked event affects current ticket:', eventData);
  
  // Determine which ticket ID to remove from our linked tickets list
  const linkedTicketIdToRemove = isSourceTicket ? eventData.linked_ticket_id : eventData.ticket_id;
  
  // Remove the linked ticket from the current ticket's linkedTickets array
  if (ticket.value.linkedTickets) {
    const ticketIndex = ticket.value.linkedTickets.findIndex(id => id === linkedTicketIdToRemove);
    if (ticketIndex !== -1) {
      ticket.value.linkedTickets.splice(ticketIndex, 1);
      console.log(`✅ TicketView: Removed linked ticket ${linkedTicketIdToRemove} from ticket (${ticket.value.linkedTickets.length} linked tickets remaining)`);
      
      // Show a brief visual indicator
      showDeviceUpdateIndicator.value = true;
      setTimeout(() => {
        showDeviceUpdateIndicator.value = false;
      }, 2000);
      
    } else {
      console.log(`🎫 TicketView: Linked ticket ${linkedTicketIdToRemove} not found in current ticket linked tickets for removal`);
    }
  } else {
    console.log('🎫 TicketView: No linked tickets in ticket to remove');
  }
};

const handleProjectAssigned = async (data: any) => {
  console.log('📁 TicketView: handleProjectAssigned called with data:', data);
  
  // Handle both event formats - the data might be nested or direct
  const eventData = data.data || data;
  console.log('📁 TicketView: Processed event data:', eventData);
  
  if (!ticket.value) {
    console.log('📁 TicketView: No ticket loaded, ignoring project assigned event');
    return;
  }
  
  if (eventData.ticket_id !== ticket.value.id) {
    console.log(`📁 TicketView: Event for different ticket (${eventData.ticket_id} vs ${ticket.value.id}), ignoring`);
    return;
  }
  
  console.log('📁 TicketView: Project assigned event matches current ticket:', eventData);
  
  try {
    // Fetch the project information to add to the ticket
    console.log(`📁 TicketView: Fetching project ${eventData.project_id} details...`);
    const project = await projectService.getProject(eventData.project_id);
    console.log('📁 TicketView: Fetched project details:', project);
    
    // Update the ticket's project information
    if (project) {
      ticket.value.project = String(project.id);
      projectDetails.value = project as any; // Cast to avoid type mismatch between different Project interfaces
      console.log(`✅ TicketView: Added project ${project.name} to ticket`);
      
      // Show a brief visual indicator
      showDeviceUpdateIndicator.value = true;
      setTimeout(() => {
        showDeviceUpdateIndicator.value = false;
      }, 2000);
    }
    
  } catch (error) {
    console.error('📁 TicketView: Error fetching assigned project:', error);
    // Fallback to full refresh if project fetch fails
    console.log('📁 TicketView: Falling back to full ticket refresh...');
    refreshTicket();
  }
};

const handleProjectUnassigned = (data: any) => {
  console.log('📁 TicketView: handleProjectUnassigned called with data:', data);
  
  // Handle both event formats - the data might be nested or direct
  const eventData = data.data || data;
  console.log('📁 TicketView: Processed event data:', eventData);
  
  if (!ticket.value) {
    console.log('📁 TicketView: No ticket loaded, ignoring project unassigned event');
    return;
  }
  
  if (eventData.ticket_id !== ticket.value.id) {
    console.log(`📁 TicketView: Event for different ticket (${eventData.ticket_id} vs ${ticket.value.id}), ignoring`);
    return;
  }
  
  console.log('📁 TicketView: Project unassigned event matches current ticket:', eventData);
  
  // Remove the project from the ticket
  if (ticket.value.project && projectDetails.value && Number(ticket.value.project) === eventData.project_id) {
    const removedProjectName = projectDetails.value.name;
    ticket.value.project = undefined;
    projectDetails.value = null;
    console.log(`✅ TicketView: Removed project ${removedProjectName} from ticket`);
    
    // Show a brief visual indicator
    showDeviceUpdateIndicator.value = true;
    setTimeout(() => {
      showDeviceUpdateIndicator.value = false;
    }, 2000);
    
  } else {
    console.log(`📁 TicketView: Project ${eventData.project_id} not found in current ticket for removal`);
  }
};
</script>

<template>
  <div class="flex-1">
    <div v-if="ticket" class="flex flex-col">
      <!-- Navigation and actions bar -->
      <div class="pt-4 px-6 flex justify-between items-center">
        <div class="flex items-center gap-4">
          <BackButton 
            v-if="ticket.project" 
            context="project" 
            :contextId="ticket.project" 
            :fallbackRoute="'/tickets'" 
          />
          <BackButton 
            v-else 
            fallbackRoute="/tickets" 
          />
          
          <!-- SSE Connection Status -->
          <div class="flex items-center gap-2 text-sm">
            <div 
              class="w-2 h-2 rounded-full"
              :class="{
                            'bg-green-400': isConnected,
            'bg-yellow-400 animate-pulse': !isConnected,
            'bg-red-400': !isConnected
              }"
            ></div>
            <span class="text-slate-400">
              {{ isConnected ? 'Live updates' : 'Connecting...' }}
            </span>
            
            <!-- Real-time Update Indicator -->
            <div v-if="showDeviceUpdateIndicator" class="flex items-center gap-1 px-2 py-1 bg-blue-600/20 rounded-md border border-blue-500/30">
              <div class="w-2 h-2 bg-blue-400 rounded-full animate-pulse"></div>
              <span class="text-blue-400 text-xs font-medium">Live Update</span>
            </div>
          </div>
        </div>
        
        <DeleteButton 
          fallbackRoute="/tickets" 
          itemName="Ticket" 
          @delete="deleteTicket" 
        />
      </div>
      
      <div class="flex flex-col gap-3 px-6 py-3 mx-auto w-full max-w-8xl">
        <!-- Grid Container -->
        <div class="grid-container">
          <!-- Details (TicketDetails and DeviceDetails) -->
          <div class="details-area flex flex-col gap-4">
            <TicketDetails 
              :ticket="ticket" 
              :created-date="formattedCreatedDate" 
              :modified-date="formattedModifiedDate"
              :selected-status="selectedStatus" 
              :selected-priority="selectedPriority"
              :status-options="STATUS_OPTIONS" 
              :priority-options="PRIORITY_OPTIONS"
              @update:selectedStatus="updateStatus" 
              @update:selectedPriority="updatePriority"
              @update:requester="updateRequester"
              @update:assignee="updateAssignee"
            />
            
            <!-- Device section -->
            <template v-if="ticket">
              <div v-if="devices.length" class="flex flex-col gap-2">
                <div class="flex items-center justify-between">
                  <h3 class="text-sm font-medium text-slate-300">Devices</h3>
                  <a 
                    href="#" 
                    @click.prevent="showDeviceModal = true" 
                    class="text-blue-500 hover:text-blue-400 text-sm hover:underline"
                  >
                    + Add device
                  </a>
                </div>
                <div class="flex flex-col gap-2">
                  <DeviceDetails
                    v-for="device in devices"
                    :key="device.id"
                    :device="device"
                    @remove="() => removeDevice(device.id)"
                    @view="navigateToDeviceView"
                    @update:name="(value) => updateDeviceField(device.id, 'name', value)"
                    @update:hostname="(value) => updateDeviceField(device.id, 'hostname', value)"
                    @update:serial_number="(value) => updateDeviceField(device.id, 'serial_number', value)"
                    @update:model="(value) => updateDeviceField(device.id, 'model', value)"
                    @update:warranty_status="(value) => updateDeviceField(device.id, 'warranty_status', value)"
                  />
                </div>
              </div>
              <div v-else>
                <a href="#" @click.prevent="showDeviceModal = true" class="block text-blue-500 hover:underline">+ Add device</a>
              </div>

              <!-- Add DeviceModal component -->
              <DeviceSelectionModal
                :show="showDeviceModal"
                :current-ticket-id="ticket?.id"
                :existing-device-ids="devices.map(d => d.id)"
                :requester-uuid="ticket?.requester"
                @close="showDeviceModal = false"
                @select-device="handleAddDevice"
              />

              <!-- Linked ticket section -->
              <div v-if="ticket.linkedTickets?.length" class="flex flex-col gap-2">
                <div class="flex items-center justify-between">
                  <h3 class="text-sm font-medium text-slate-300">Linked Tickets</h3>
                  <a 
                    href="#" 
                    @click.prevent="showLinkedTicketModal = true" 
                    class="text-blue-500 hover:text-blue-400 text-sm hover:underline"
                  >
                    + Add linked ticket
                  </a>
                </div>
                <div class="flex flex-col gap-2">
                  <LinkedTicketPreview
                    v-for="linkedId in ticket.linkedTickets"
                    :key="linkedId"
                    :linked-ticket-id="linkedId"
                    :current-ticket-id="ticket.id"
                    @unlink="() => unlinkTicket(linkedId)"
                    @view="() => {}"
                  />
                </div>
              </div>
              <div v-else>
                <a href="#" @click.prevent="showLinkedTicketModal = true" class="block text-blue-500 hover:underline">+ Add linked ticket</a>
              </div>

              <!-- Project section -->
              <div class="flex flex-col gap-2">
                <div v-if="ticket.project" class="flex items-center justify-between">
                  <h3 class="text-sm font-medium text-slate-300">Project</h3>
                  <a 
                    href="#" 
                    @click.prevent="showProjectModal = true" 
                    class="text-blue-500 hover:text-blue-400 text-sm hover:underline"
                  >
                    Change project
                  </a>
                </div>

                <div v-if="ticket.project && projectDetails">
                  <!-- Project Info -->
                  <ProjectInfo
                    :project="projectDetails"
                    :project-id="ticket.project"
                    @view="viewProject(ticket.project!)"
                    @remove="removeFromProject"
                  />
                </div>

                <div v-else>
                  <a 
                    href="#" 
                    @click.prevent="showProjectModal = true" 
                    class="block text-blue-500 hover:underline"
                  >
                    + Add to project
                  </a>
                </div>

                <!-- Project Selection Modal -->
                <ProjectSelectionModal
                  :show="showProjectModal"
                  :current-project-id="ticket.project ? Number(ticket.project) : undefined"
                  @close="showProjectModal = false"
                  @select-project="handleAddToProject"
                />
              </div>
            </template>
          </div>

          <!-- Collaborative Ticket Article Body -->
          <div class="article-area rounded-xl">
            <CollaborativeTicketArticle 
              :initial-content="ticket.article_content || ''"
              :ticket-id="ticket.id"
            />
          </div>

          <!-- Comments and Attachments -->
          <div class="comments-area rounded-xl">
            <CommentsAndAttachments 
              :comments="comments"
              :current-user="authStore.user?.uuid || 'Unknown User'"
              :recently-added-comment-ids="recentlyAddedCommentIds"
              @add-comment="handleAddComment"
              @delete-attachment="handleDeleteAttachment"
              @delete-comment="handleDeleteComment"
            />
          </div>
        </div>
      </div>
    </div>

    <div v-else class="p-6 text-center text-gray-400">
      Loading ticket...
    </div>

    <LinkedTicketModal 
      v-if="ticket"
      :show="showLinkedTicketModal"
      :current-ticket-id="ticket.id"
      :existing-linked-tickets="ticket.linkedTickets"
      @close="showLinkedTicketModal = false"
      @select-ticket="handleLinkTicket"
    />
  </div>
</template>

<style scoped>
.grid-container {
  display: grid;
  grid-template-columns: 1fr;
  grid-template-rows: auto auto 1fr;
  grid-template-areas: "details" "article" "comments";
  gap: 1rem;
  min-height: calc(100vh - 140px); /* Changed from height to min-height */

  @media (min-width: 1280px) {
    grid-template-columns: minmax(400px, 1fr) minmax(0, 2fr);
    grid-template-rows: auto 1fr; /* Add explicit rows */
    grid-template-areas:
      "details article"
      "comments article";
  }

  @media (min-width: 1860px) {
    grid-template-columns: minmax(400px, 1fr) minmax(0, 2fr) minmax(400px, 1fr);
    grid-template-rows: 1fr; /* Single row at large screens */
    grid-template-areas: "details article comments";
  }
}

.details-area {
  grid-area: details;
  max-height: 100%;
  overflow-y: auto;
}

.article-area {
  grid-area: article;
  display: flex;
  flex-direction: column;
  overflow: visible; /* Allow content to be fully visible */
  min-height: fit-content; /* Let it grow based on content */
}

.comments-area {
  grid-area: comments;
  max-height: 100%;
  overflow-y: auto;
}
</style>