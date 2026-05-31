<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { computed, ref } from "vue";
import { useFluent } from 'fluent-vue';
import UserAvatar from "@/components/UserAvatar.vue";
import VoiceRecorder from "@/components/ticketComponents/VoiceRecorder.vue";
import AttachmentPreview from "@/components/ticketComponents/AttachmentPreview.vue";
import SectionCard from "@/components/common/SectionCard.vue";
import SimpleEditor from "@/components/common/SimpleEditor.vue";
import MarkdownRenderer from "@/components/common/MarkdownRenderer.vue";
import CommentContent from "@/components/ticketComponents/CommentContent.vue";
import { sanitiseHtml } from "@/composables/useSanitise";
import CannedResponsePicker from "@/components/ticketComponents/CannedResponsePicker.vue";
import uploadService from "@/services/uploadService";
import { convertToAuthenticatedPath } from '@/services/fileService';
import { useTicketDraftsStore } from "@/stores/ticketDrafts";
import { useTicketUiStore } from "@/stores/ticketUi";

// Local re-export of the canonical types so this component can use
// them without churn through every consumer.
import type { CommentWithAttachments } from '@/types/comment';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const props = defineProps<{
    comments: CommentWithAttachments[];
    currentUser: string;
    /** Ticket id, required for the comment composer's draft to
     *  persist across navigation and refresh (via
     *  `useTicketDraftsStore`). When undefined the composer falls
     *  back to local refs and drafts evaporate on unmount, used
     *  by the rare callers that mount this composer outside a
     *  ticket context. */
    ticketId?: number;
    recentlyAddedCommentIds?: Set<number>;
    /** Optional template context for the canned-response picker —
        `{{ticket_id}}`, `{{customer_name}}` etc. substitute at
        insert time. Omit when the composer isn't on a ticket. */
    templateVars?: {
        ticket_id?: number | string;
        ticket_title?: string;
        customer_name?: string;
        tech_name?: string;
        app_name?: string;
    };
}>();

const fileInputRef = ref<HTMLInputElement | null>(null);
const showRecordingInterface = ref(false);
const isDraggingFile = ref(false);
const conversionMessage = ref<string | null>(null);

// Composer state lives in Pinia stores keyed by ticket id so it
// survives nav-away/back and (for the text draft) survives refresh.
// Local fallback refs cover the no-ticket case and keep TypeScript
// happy when `ticketId` is undefined.
const draftsStore = useTicketDraftsStore();
const uiStore = useTicketUiStore();

const localContent = ref<string>("");
const localAttachments = ref<File[]>([]);
const localIsInternal = ref<boolean>(false);

const newCommentContent = computed<string>({
    get: () =>
        props.ticketId !== undefined
            ? draftsStore.getDraft(props.ticketId).content
            : localContent.value,
    set: (value) => {
        if (props.ticketId !== undefined) {
            const current = draftsStore.getDraft(props.ticketId);
            draftsStore.setDraft(props.ticketId, { ...current, content: value });
        } else {
            localContent.value = value;
        }
    },
});

// Internal notes are tech-to-tech: hidden from requesters and not
// relayed back through the originating channel. Defaults off so the
// common path (public reply) stays the single-click case.
const isInternal = computed<boolean>({
    get: () =>
        props.ticketId !== undefined
            ? draftsStore.getDraft(props.ticketId).isInternal
            : localIsInternal.value,
    set: (value) => {
        if (props.ticketId !== undefined) {
            const current = draftsStore.getDraft(props.ticketId);
            draftsStore.setDraft(props.ticketId, { ...current, isInternal: value });
        } else {
            localIsInternal.value = value;
        }
    },
});

const newAttachments = computed<File[]>({
    get: () =>
        props.ticketId !== undefined
            ? uiStore.getAttachments(props.ticketId)
            : localAttachments.value,
    set: (value) => {
        if (props.ticketId !== undefined) {
            uiStore.setAttachments(props.ticketId, value);
        } else {
            localAttachments.value = value;
        }
    },
});

const emit = defineEmits<{
    (
        e: "addComment",
        value: {
            content: string;
            user_uuid: string;
            files: File[];
            is_internal: boolean;
        },
    ): void;
    (
        e: "deleteAttachment",
        value: { commentId: number; attachmentIndex: number },
    ): void;
    (e: "deleteComment", value: number): void;
}>();

/**
 * Check if HTML content has any actual text (not just empty tags)
 */
const hasTextContent = (html: string): boolean => {
    if (!html) return false;
    // Create a temporary element to extract text content
    const temp = document.createElement('div');
    temp.innerHTML = html;
    return temp.textContent?.trim().length > 0;
};

// Template vars for the canned-response picker. `templateVars` prop
// is optional; default to an empty object so tokens like
// `{{customer_name}}` render as-is rather than erroring.
const cannedResponseVars = computed(() => props.templateVars ?? {});

// Thread-pivot filter: lets a tech narrow the comment list to the
// public conversation (what the requester sees) or the internal
// thread (the team's working notes) without leaving the ticket.
// Print layout deliberately ignores this so a printed record stays
// complete.
type CommentVisibilityFilter = 'all' | 'public' | 'internal';
const commentFilter = ref<CommentVisibilityFilter>('all');

const internalCommentCount = computed(
    () => props.comments.filter((c) => c.is_internal).length,
);
const publicCommentCount = computed(
    () => props.comments.length - internalCommentCount.value,
);

const filteredComments = computed(() => {
    if (commentFilter.value === 'public') {
        return props.comments.filter((c) => !c.is_internal);
    }
    if (commentFilter.value === 'internal') {
        return props.comments.filter((c) => c.is_internal);
    }
    return props.comments;
});

// Inserts rendered canned-response text into the composer. SimpleEditor's
// v-model is HTML; plain text with newlines is rendered by wrapping in
// paragraphs — `\n\n` becomes a paragraph break, single `\n` a `<br>`.
function insertCannedResponse(text: string) {
    const html = text
        .split(/\n\n+/)
        .map((para) => `<p>${para.replace(/\n/g, "<br>")}</p>`)
        .join("");
    // Append rather than replace — the tech may have started typing
    // context before pulling a template.
    newCommentContent.value = newCommentContent.value
        ? `${newCommentContent.value}${html}`
        : html;
}

const addComment = () => {
    if (!hasTextContent(newCommentContent.value) && newAttachments.value.length === 0)
        return;

    emit("addComment", {
        content: newCommentContent.value,
        user_uuid: props.currentUser,
        files: newAttachments.value,
        is_internal: isInternal.value,
    });

    // Reset form — including the internal flag, so the next reply
    // defaults back to public and a tech has to opt in each time.
    newCommentContent.value = "";
    newAttachments.value = [];
    isInternal.value = false;
};

const processFiles = async (files: File[]): Promise<File[]> => {
    const processedFiles: File[] = [];
    for (const file of files) {
        try {
            // Convert HEIC to WebP if it's an image
            const processedFile = file.type.startsWith("image/")
                ? await uploadService.convertHeicToJpeg(file, (message) => {
                      conversionMessage.value = message;
                      // Auto-clear success message after 2 seconds
                      if (message.includes("successful")) {
                          setTimeout(() => {
                              conversionMessage.value = null;
                          }, 2000);
                      }
                  })
                : file;
            processedFiles.push(processedFile);
        } catch (error) {
            console.error(`Error processing file ${file.name}:`, error);
            conversionMessage.value = null;
            // Still add the original file if conversion fails
            processedFiles.push(file);
        }
    }
    return processedFiles;
};

const handleFileUpload = async (event: Event) => {
    const input = event.target as HTMLInputElement;
    if (input.files) {
        const files = Array.from(input.files);

        // Process non-audio files (convert HEIC images if needed)
        const nonAudioFiles = files.filter((file) => !file.type.startsWith("audio/"));
        const processedFiles = await processFiles(nonAudioFiles);

        // Audio files go directly to attachments (no special processing needed)
        const audioFiles = files.filter((file) => file.type.startsWith("audio/"));

        // Reassign rather than mutate-in-place: `newAttachments`
        // is now a writable computed backed by `useTicketUiStore`,
        // and a `.push()` would skip the setter that writes to
        // the store.
        newAttachments.value = [...newAttachments.value, ...processedFiles, ...audioFiles];
    }
    // Reset input so the same file can be selected again
    if (input) input.value = '';
};

const triggerFileUpload = () => {
    fileInputRef.value?.click();
};

const startVoiceRecording = () => {
    showRecordingInterface.value = true;
};

const handleRecordingComplete = (recording: {
    blob: Blob;
    duration: number;
    transcription?: string;
}) => {
    console.log('[CommentsAndAttachments] Recording complete, transcription:', recording.transcription);

    // Auto-stage the voice note as an attachment
    const fileName = `${t('ticket-comments-voice-note-filename', { date: formatDate(new Date(), 'MMM d, yyyy') })}.webm`;
    const audioFile = new File([recording.blob], fileName, {
        type: recording.blob.type,
    }) as File & { _transcription?: string };

    if (recording.transcription) {
        (audioFile as any)._transcription = recording.transcription;
        console.log('[CommentsAndAttachments] Attached transcription to file');
    }

    newAttachments.value = [...newAttachments.value, audioFile];
    console.log('[CommentsAndAttachments] File _transcription:', (audioFile as any)._transcription);
    showRecordingInterface.value = false;
};

const handleRecordingCancel = () => {
    showRecordingInterface.value = false;
};

const deleteAttachment = (commentId: number, attachmentIndex: number) => {
    emit("deleteAttachment", { commentId, attachmentIndex });
};

const deleteComment = (commentId: number) => {
    emit("deleteComment", commentId);
};


/**
 * Comment timestamps include the time as well as the date — without it
 * a busy ticket's timeline reads as several entries on the same day
 * with no way to tell their order at a glance. `formatDateTime` uses
 * the user's locale (en-US default) for "Apr 29, 2026, 02:13 PM".
 */
const formattedDate = (dateString: string): string => {
    return formatDateTime(dateString);
};

// Check if comment has real text content (not just empty HTML or placeholder)
const hasRealContent = (comment: CommentWithAttachments): boolean => {
    if (!hasTextContent(comment.content)) return false;
    // Also check for placeholder text
    const temp = document.createElement('div');
    temp.innerHTML = comment.content || '';
    const text = temp.textContent?.trim().toLowerCase() || '';
    return text !== 'attachment added';
};

// Check if comment is audio-only (no text, single audio attachment)
const isAudioOnlyComment = (comment: CommentWithAttachments): boolean => {
    if (hasRealContent(comment)) return false;
    if (!comment.attachments || comment.attachments.length !== 1) return false;
    const name = comment.attachments[0]?.name?.toLowerCase() || '';
    const audioExtensions = ['.mp3', '.wav', '.ogg', '.m4a', '.webm', '.aac'];
    return audioExtensions.some(ext => name.endsWith(ext)) || name.includes('voice note');
};

// Get display name for audio - "Voice Message" for voice notes
const getAudioDisplayName = (filename: string): string => {
    if (!filename) return t('ticket-comments-audio-default');
    const lower = filename.toLowerCase();
    if (lower.startsWith('voice note') || lower.startsWith('voicenote')) {
        return t('ticket-comments-audio-voice-message');
    }
    return filename;
};

const handleDragEnter = (event: DragEvent) => {
    event.preventDefault();
    event.stopPropagation();
    isDraggingFile.value = true;
};

const handleDragLeave = (event: DragEvent) => {
    event.preventDefault();
    event.stopPropagation();
    const target = event.currentTarget as HTMLElement;
    const relatedTarget = event.relatedTarget as Node;
    if (!target?.contains(relatedTarget)) {
        isDraggingFile.value = false;
    }
};

const handleDragOver = (event: DragEvent) => {
    event.preventDefault();
    event.stopPropagation();
};

const handleDrop = async (event: DragEvent) => {
    event.preventDefault();
    event.stopPropagation();
    isDraggingFile.value = false;

    if (!event.dataTransfer?.files.length) return;

    const files = Array.from(event.dataTransfer.files);

    // Process non-audio files (convert HEIC images if needed)
    const nonAudioFiles = files.filter((file) => !file.type.startsWith("audio/"));
    const processedFiles = await processFiles(nonAudioFiles);

    // Audio files go directly to attachments
    const audioFiles = files.filter((file) => file.type.startsWith("audio/"));

    newAttachments.value = [...newAttachments.value, ...processedFiles, ...audioFiles];
};

// Image files pasted into the composer (screenshots from macOS
// Cmd-Shift-Ctrl-4, Windows Snipping Tool, Linux Flameshot, etc.).
// SimpleEditor extracts the files inside ProseMirror's `handlePaste`
// hook so PM doesn't also try to insert the bitmap inline as a data:
// URL, then emits them up to here as a plain File[].
//
// Plain text and image URLs are not emitted; those fall through to
// PM's default paste handling so "https://example.com/cat.png" still
// produces a clickable link instead of a CORS-fraught download.
const handlePastedFiles = async (files: File[]) => {
    if (files.length === 0) return;

    // Pasted screenshots typically arrive as "image.png" without a useful
    // name; rename to a timestamp so multiple pastes in the same composer
    // don't collide on the server side, and so the attachment chip carries
    // a recognisable label.
    const stamped = files.map((file) => {
        const ext = file.type.split("/")[1] || "png";
        const ts = new Date().toISOString().replace(/[:.]/g, "-");
        return new File([file], `pasted-${ts}.${ext}`, { type: file.type });
    });

    const processed = await processFiles(stamped);
    newAttachments.value = [...newAttachments.value, ...processed];
};
</script>

<template>
    <!--
      Flush content padding (`content-padding=""`): the section's own
      `p-3` is dropped so each interior block can manage its own
      breathing room. The composer keeps a comfortable `p-3`; the
      comment list uses tighter `px-2` so the email iframe inside
      gets closer to the section's edge. The comment-row bordered
      cards keep their own `p-3` for visual chunking.
    -->
    <SectionCard content-padding="">
        <template #title>{{ $t('ticket-comments-section-title') }}</template>

        <template #default>
            <div class="flex flex-col">
                <!-- Conversion Status Message (hidden on print) -->
                <div
                    v-if="conversionMessage"
                    class="print:hidden mx-3 mt-3 bg-accent/20 border border-accent/50 text-accent px-4 py-2 rounded-lg text-sm flex items-center gap-2"
                >
                    <svg
                        v-if="conversionMessage.includes('Converting')"
                        class="w-4 h-4 animate-spin"
                        fill="none"
                        viewBox="0 0 24 24"
                    >
                        <circle
                            class="opacity-25"
                            cx="12"
                            cy="12"
                            r="10"
                            stroke="currentColor"
                            stroke-width="4"
                        ></circle>
                        <path
                            class="opacity-75"
                            fill="currentColor"
                            d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                        ></path>
                    </svg>
                    <svg
                        v-else
                        class="w-4 h-4"
                        fill="currentColor"
                        viewBox="0 0 20 20"
                    >
                        <path
                            fill-rule="evenodd"
                            d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z"
                            clip-rule="evenodd"
                        />
                    </svg>
                    {{ conversionMessage }}
                </div>

                <!--
                  Composer runs edge-to-edge with the section card
                  and uses a single `border-b` as a divider against
                  the comment list below — no rounded corners, no
                  side borders. The shape reads as a section header
                  rather than a nested card.
                -->
                <div
                    class="print:hidden border-b relative p-3 transition-colors"
                    :class="
                        isInternal
                            ? 'bg-status-warning-bg/40 border-status-warning-border/60'
                            : 'bg-surface border-default'
                    "
                    @dragenter="handleDragEnter"
                    @dragleave="handleDragLeave"
                    @dragover="handleDragOver"
                    @drop="handleDrop"
                >
                    <!-- Drag overlay with pointer-events-none to avoid capturing mouse events -->
                    <div
                        v-if="isDraggingFile"
                        class="absolute inset-0 bg-accent/10 border-2 border-accent border-dashed rounded-lg flex items-center justify-center pointer-events-none"
                        style="z-index: 30"
                    >
                        <div
                            class="bg-surface-alt rounded-lg px-4 py-2 text-accent flex items-center gap-2"
                        >
                            <svg
                                xmlns="http://www.w3.org/2000/svg"
                                class="h-5 w-5"
                                viewBox="0 0 20 20"
                                fill="currentColor"
                            >
                                <path
                                    d="M5.5 13a3.5 3.5 0 01-.369-6.98 4 4 0 117.753-1.977A4.5 4.5 0 1113.5 13H11V9.413l1.293 1.293a1 1 0 001.414-1.414l-3-3a1 1 0 00-1.414 0l-3 3a1 1 0 001.414 1.414L9 9.414V13H5.5z"
                                />
                                <path d="M9 13h2v5a1 1 0 11-2 0v-5z" />
                            </svg>
                            {{ $t('ticket-comments-drop-files') }}
                        </div>
                    </div>

                    <form
                        @submit.prevent="addComment"
                        class="flex flex-col gap-2"
                    >
                        <div
                            v-if="isInternal"
                            class="flex items-center gap-2 text-xs font-medium text-status-warning"
                        >
                            <svg
                                xmlns="http://www.w3.org/2000/svg"
                                class="h-4 w-4 flex-shrink-0"
                                viewBox="0 0 20 20"
                                fill="currentColor"
                                aria-hidden="true"
                            >
                                <path
                                    fill-rule="evenodd"
                                    d="M10 18a8 8 0 100-16 8 8 0 000 16zm0-13a1 1 0 011 1v4a1 1 0 11-2 0V6a1 1 0 011-1zm0 9a1 1 0 100-2 1 1 0 000 2z"
                                    clip-rule="evenodd"
                                />
                            </svg>
                            <span>{{ $t('ticket-comments-internal-banner') }}</span>
                        </div>
                        <SimpleEditor
                            v-model="newCommentContent"
                            :placeholder="isInternal ? $t('ticket-comments-placeholder-internal') : $t('ticket-comments-placeholder-public')"
                            min-height="60px"
                            max-height="200px"
                            @submit="addComment"
                            @paste-files="handlePastedFiles"
                        />

                        <!-- New attachments -->
                        <div
                            v-if="newAttachments.length > 0"
                            class="flex flex-wrap gap-2"
                        >
                            <AttachmentPreview
                                v-for="(file, index) in newAttachments"
                                :key="index"
                                :attachment="{
                                    url: uploadService.createPreviewUrl(file),
                                    name: file.name,
                                    transcription: (file as any)._transcription,
                                }"
                                :author="props.currentUser"
                                :timestamp="
                                    formattedDate(new Date().toISOString())
                                "
                                :is-new="true"
                                :show-delete="true"
                                @delete="newAttachments = newAttachments.filter((_, i) => i !== index)"
                                @submit="addComment"
                            />
                        </div>

                        <!-- Voice Recorder -->
                        <VoiceRecorder
                            v-if="showRecordingInterface"
                            @recording-complete="handleRecordingComplete"
                            @cancel="handleRecordingCancel"
                        />

                        <!-- Hidden file input -->
                        <input
                            ref="fileInputRef"
                            type="file"
                            @change="handleFileUpload"
                            multiple
                            class="hidden"
                        />

                        <!--
                          Single action row: secondary actions (mic /
                          attach / canned-responses) on the left,
                          visibility toggle + submit on the right.
                          Mirrors the composer pattern in Front,
                          Help Scout and Linear — secondary actions
                          left of a flex spacer, primary actions right.

                          The visibility segmented control sits next
                          to the submit button so the mode is set in
                          the same eye-line as the action it gates;
                          submit colour follows the mode (accent for
                          public reply, warning for internal note)
                          so the next click's effect is obvious.

                          Wraps to a second line on narrow viewports
                          (`flex-wrap`) rather than overflowing — the
                          right-side group stays grouped because the
                          spacer collapses first.
                        -->
                        <div class="flex flex-wrap items-center gap-2">
                            <!-- Voice Recording Button -->
                            <button
                                type="button"
                                @click="startVoiceRecording"
                                class="touch-target sm:h-9 px-3 sm:px-2.5 bg-surface-alt border border-default text-secondary rounded-md hover:bg-surface-hover hover:text-primary transition-colors flex items-center justify-center"
                                :class="{ 'text-error': showRecordingInterface }"
                                :aria-label="$t('ticket-comments-record-voice')"
                                :title="$t('ticket-comments-record-voice')"
                            >
                                <svg
                                    xmlns="http://www.w3.org/2000/svg"
                                    class="h-5 w-5"
                                    viewBox="0 0 20 20"
                                    fill="currentColor"
                                >
                                    <path
                                        fill-rule="evenodd"
                                        d="M7 4a3 3 0 016 0v4a3 3 0 11-6 0V4zm4 10.93A7.001 7.001 0 0017 8a1 1 0 10-2 0A5 5 0 015 8a1 1 0 00-2 0 7.001 7.001 0 006 6.93V17H6a1 1 0 100 2h8a1 1 0 100-2h-3v-2.07z"
                                        clip-rule="evenodd"
                                    />
                                </svg>
                            </button>
                            <!-- File Upload Button -->
                            <button
                                type="button"
                                @click="triggerFileUpload"
                                class="touch-target sm:h-9 px-3 sm:px-2.5 bg-surface-alt border border-default text-secondary rounded-md hover:bg-surface-hover hover:text-primary transition-colors flex items-center justify-center"
                                :aria-label="$t('ticket-comments-upload-file')"
                                :title="$t('ticket-comments-upload-file')"
                            >
                                <svg
                                    xmlns="http://www.w3.org/2000/svg"
                                    class="h-5 w-5"
                                    viewBox="0 0 20 20"
                                    fill="currentColor"
                                >
                                    <path
                                        fill-rule="evenodd"
                                        d="M8 4a3 3 0 00-3 3v4a5 5 0 0010 0V7a1 1 0 112 0v4a7 7 0 11-14 0V7a5 5 0 0110 0v4a3 3 0 11-6 0V7a1 1 0 012 0v4a1 1 0 102 0V7a3 3 0 00-3-3z"
                                        clip-rule="evenodd"
                                    />
                                </svg>
                            </button>
                            <!-- Canned Responses Picker -->
                            <CannedResponsePicker
                                :vars="cannedResponseVars"
                                :ticket-id="ticketId"
                                @insert="insertCannedResponse"
                            />

                            <!-- Pushes the submit cluster to the right edge. -->
                            <div class="flex-1"></div>

                            <!-- Public reply / Internal note segmented control -->
                            <div
                                class="flex items-center rounded-md bg-surface-alt border border-default p-0.5 text-xs font-medium"
                                role="group"
                                :aria-label="$t('ticket-comments-visibility-group')"
                            >
                                <button
                                    type="button"
                                    @click="isInternal = false"
                                    :class="[
                                        'px-2.5 py-1 rounded transition-colors',
                                        !isInternal
                                            ? 'bg-accent text-on-accent'
                                            : 'text-secondary hover:text-primary'
                                    ]"
                                    :title="$t('ticket-comments-public-reply-title')"
                                >
                                    {{ $t('ticket-comments-public-reply') }}
                                </button>
                                <button
                                    type="button"
                                    @click="isInternal = true"
                                    :class="[
                                        'px-2.5 py-1 rounded transition-colors',
                                        isInternal
                                            ? 'bg-status-warning text-white'
                                            : 'text-secondary hover:text-primary'
                                    ]"
                                    :title="$t('ticket-comments-internal-note-title')"
                                >
                                    {{ $t('ticket-comments-internal-note') }}
                                </button>
                            </div>

                            <!-- Submit. Colour follows the visibility
                                 mode so the click's effect matches the
                                 segmented control immediately above. -->
                            <button
                                type="submit"
                                :class="[
                                    'h-9 px-4 rounded-md text-white text-sm font-medium hover:opacity-90 transition-colors',
                                    isInternal ? 'bg-status-warning' : 'bg-accent'
                                ]"
                            >
                                {{ isInternal ? $t('ticket-comments-submit-note') : $t('ticket-comments-submit-reply') }}
                            </button>
                        </div>
                    </form>
                </div>

                <!-- List of Comments - Screen layout -->
                <div
                    v-if="props.comments.length > 0"
                    class="print:hidden flex flex-col gap-2 px-2 py-3"
                >
                    <!-- Visibility pivot: hidden when the ticket has
                         no internal notes yet (nothing to filter).
                         Counts give a sense of thread shape before
                         the filter is engaged. -->
                    <div
                        v-if="internalCommentCount > 0"
                        class="flex items-center gap-1 text-xs"
                        role="group"
                        :aria-label="$t('ticket-comments-filter-group')"
                    >
                        <button
                            type="button"
                            class="px-2 py-1 rounded transition-colors"
                            :class="
                                commentFilter === 'all'
                                    ? 'bg-surface-alt text-primary font-medium'
                                    : 'text-tertiary hover:text-primary'
                            "
                            @click="commentFilter = 'all'"
                        >
                            {{ $t('ticket-comments-filter-all', { count: props.comments.length }) }}
                        </button>
                        <button
                            type="button"
                            class="px-2 py-1 rounded transition-colors"
                            :class="
                                commentFilter === 'public'
                                    ? 'bg-surface-alt text-primary font-medium'
                                    : 'text-tertiary hover:text-primary'
                            "
                            @click="commentFilter = 'public'"
                        >
                            {{ $t('ticket-comments-filter-public', { count: publicCommentCount }) }}
                        </button>
                        <button
                            type="button"
                            class="px-2 py-1 rounded transition-colors"
                            :class="
                                commentFilter === 'internal'
                                    ? 'bg-status-warning-bg text-status-warning font-medium'
                                    : 'text-tertiary hover:text-primary'
                            "
                            @click="commentFilter = 'internal'"
                        >
                            {{ $t('ticket-comments-filter-internal', { count: internalCommentCount }) }}
                        </button>
                    </div>
                    <div
                        v-for="comment in filteredComments"
                        :key="comment.id"
                        class="flex flex-col gap-2 sm:gap-3 p-2 sm:p-3 rounded-lg border transition-all duration-300"
                        :class="[
                            props.recentlyAddedCommentIds?.has(comment.id)
                                ? 'bg-accent/20 border-accent/50 animate-pulse'
                                : comment.is_internal
                                    ? 'bg-status-warning-bg/30 border-status-warning-border/50'
                                    : 'bg-surface-alt border-subtle',
                        ]"
                    >
                        <!-- Mobile: Compact header with avatar, name, date, and actions inline -->
                        <!-- Desktop: Avatar on left, content beside it -->
                        <div class="flex flex-col sm:flex-row gap-2">
                            <!-- Header row: avatar, name/date, actions -->
                            <div class="flex items-center gap-2 sm:hidden">
                                <UserAvatar
                                    :uuid="comment.user?.uuid || comment.user_uuid"
                                    :fallbackName="comment.user?.name"
                                    :fallbackAvatar="comment.user?.avatar_thumb || comment.user?.avatar_url"
                                    :showName="false"
                                    size="sm"
                                    class="flex-shrink-0"
                                />
                                <div class="flex-1 min-w-0">
                                    <span class="text-sm text-primary font-medium truncate">
                                        {{ comment.user?.name || comment.user_uuid }}
                                    </span>
                                    <span
                                        v-if="comment.is_internal"
                                        class="ml-1.5 inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-semibold uppercase tracking-wide bg-status-warning text-white"
                                    >
                                        {{ $t('ticket-comments-badge-internal') }}
                                    </span>
                                    <span
                                        v-if="comment.channel_metadata?.forwarded_by_user_uuid"
                                        class="ml-1.5 inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-semibold uppercase tracking-wide bg-accent-muted text-accent"
                                        :title="$t('ticket-comments-badge-forwarded-title')"
                                    >
                                        {{ $t('ticket-comments-badge-forwarded') }}
                                    </span>
                                    <span class="text-xs text-tertiary block">
                                        {{ formattedDate(comment.createdAt ?? comment.created_at) }}
                                    </span>
                                </div>
                                <!-- Mobile action buttons (hidden on print) -->
                                <div class="print:hidden flex items-center gap-1 flex-shrink-0">
                                    <a
                                        v-if="isAudioOnlyComment(comment)"
                                        :href="convertToAuthenticatedPath(comment.attachments?.[0]?.url ?? '')"
                                        :download="comment.attachments?.[0]?.name"
                                        target="_blank"
                                        class="inline-flex items-center justify-center touch-target text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
                                        :title="$t('ticket-comments-action-download')"
                                        @click.stop
                                    >
                                        <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
                                            <path fill-rule="evenodd" d="M3 17a1 1 0 011-1h12a1 1 0 110 2H4a1 1 0 01-1-1zm3.293-7.707a1 1 0 011.414 0L9 10.586V3a1 1 0 112 0v7.586l1.293-1.293a1 1 0 111.414 1.414l-3 3a1 1 0 01-1.414 0l-3-3a1 1 0 010-1.414z" clip-rule="evenodd" />
                                        </svg>
                                    </a>
                                    <button
                                        v-if="hasRealContent(comment) || isAudioOnlyComment(comment)"
                                        type="button"
                                        @click="isAudioOnlyComment(comment) ? deleteAttachment(comment.id, 0) : deleteComment(comment.id)"
                                        class="inline-flex items-center justify-center touch-target text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
                                        :title="isAudioOnlyComment(comment) ? $t('ticket-comments-action-delete-voice') : $t('ticket-comments-action-delete-comment')"
                                    >
                                        <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
                                            <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
                                        </svg>
                                    </button>
                                </div>
                            </div>

                            <!-- Mobile: Full-width content below header -->
                            <div class="sm:hidden w-full">
                                <CommentContent
                                    v-if="hasRealContent(comment)"
                                    :content="comment.content"
                                    :content-format="comment.content_format"
                                    :render-kind="comment.render_kind"
                                    :new-content="comment.new_content"
                                    :quoted-content="comment.quoted_content"
                                    :has-raw-source="comment.has_raw_source"
                                    :comment-id="comment.id"
                                />
                                <p v-else-if="isAudioOnlyComment(comment)" class="text-primary text-sm">
                                    {{ getAudioDisplayName(comment.attachments?.[0]?.name ?? '') }}
                                </p>
                            </div>

                            <!-- Desktop layout. Outer column stacks
                                 [header] over [body] so the body spans
                                 the full content width — the email
                                 iframe inside is not squeezed under the
                                 avatar like a Gmail-style indent.
                                 The header itself is a flex row with
                                 the avatar on the left and a two-line
                                 name-and-meta cluster on the right of
                                 the avatar. Action buttons sit at the
                                 right edge of the first meta line. -->
                            <div class="hidden sm:flex sm:flex-col gap-2 flex-1 min-w-0">
                                <div class="flex items-start gap-2 min-w-0">
                                    <UserAvatar
                                        :uuid="comment.user?.uuid || comment.user_uuid"
                                        :fallbackName="comment.user?.name"
                                        :fallbackAvatar="comment.user?.avatar_thumb || comment.user?.avatar_url"
                                        :showName="false"
                                        size="sm"
                                        class="flex-shrink-0 mt-0.5"
                                    />
                                    <div class="flex-1 min-w-0 flex flex-col gap-1">
                                        <!--
                                          Row 1 uses `justify-between` to split
                                          two flex groups: a clustered left
                                          group (name + Internal badge + date)
                                          and a right group of action buttons.
                                          The split is structural rather than
                                          relying on a single `ml-auto`, which
                                          makes the rule for "actions hug the
                                          right edge" obvious from the markup
                                          alone — no implicit space-filler.

                                          Negative margins on the right group
                                          (`-mr-[14px]` mobile, `-mr-[18px]`
                                          desktop) cancel the comment row's
                                          outer padding (8 / 12 px) plus the
                                          trailing button's `p-1.5` interior
                                          padding (6 px) so the SVG icon ends
                                          flush with the row's border.
                                        -->
                                    <div class="flex items-center justify-between gap-2 min-w-0">
                                        <div class="flex items-center gap-2 min-w-0">
                                            <span class="text-sm text-primary font-medium truncate">
                                                {{ comment.user?.name || comment.user_uuid }}
                                            </span>
                                            <span
                                                v-if="comment.is_internal"
                                                class="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-semibold uppercase tracking-wide bg-status-warning text-white flex-shrink-0"
                                            >
                                                Internal
                                            </span>
                                            <span class="text-xs text-tertiary whitespace-nowrap flex-shrink-0">
                                                {{ formattedDate(comment.createdAt ?? comment.created_at) }}
                                            </span>
                                        </div>
                                        <div class="print:hidden flex items-center gap-1 flex-shrink-0 -mr-[14px] sm:-mr-[18px]">
                                            <a
                                                v-if="isAudioOnlyComment(comment)"
                                                :href="convertToAuthenticatedPath(comment.attachments?.[0]?.url ?? '')"
                                                :download="comment.attachments?.[0]?.name"
                                                target="_blank"
                                                class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
                                                :title="$t('ticket-comments-action-download')"
                                                @click.stop
                                            >
                                                <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
                                                    <path fill-rule="evenodd" d="M3 17a1 1 0 011-1h12a1 1 0 110 2H4a1 1 0 01-1-1zm3.293-7.707a1 1 0 011.414 0L9 10.586V3a1 1 0 112 0v7.586l1.293-1.293a1 1 0 111.414 1.414l-3 3a1 1 0 01-1.414 0l-3-3a1 1 0 010-1.414z" clip-rule="evenodd" />
                                                </svg>
                                            </a>
                                            <button
                                                v-if="hasRealContent(comment) || isAudioOnlyComment(comment)"
                                                type="button"
                                                @click="isAudioOnlyComment(comment) ? deleteAttachment(comment.id, 0) : deleteComment(comment.id)"
                                                class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
                                                :title="isAudioOnlyComment(comment) ? $t('ticket-comments-action-delete-voice') : $t('ticket-comments-action-delete-comment')"
                                            >
                                                <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
                                                    <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
                                                </svg>
                                            </button>
                                        </div>
                                    </div>
                                    <!--
                                        Row 2: secondary identity line. Renders
                                        when the comment came from a channel
                                        (`from_address` stamped by the inbound
                                        pipeline) or carries the Forwarded flag.
                                        The address is the customer's email for
                                        IMAP / mailto channels; future chat
                                        adapters can stamp their equivalent
                                        identifier in the same field.
                                    -->
                                    <div
                                        v-if="comment.from_address || comment.channel_metadata?.forwarded_by_user_uuid"
                                        class="flex items-center gap-2 -mt-1 min-w-0"
                                    >
                                        <!-- Forwarded badge leads the row so
                                             the visual hierarchy reads
                                             "channel context, then who" —
                                             the same convention Front and
                                             Help Scout use for inbound-email
                                             metadata. `break-all` on the
                                             address wraps long values rather
                                             than ellipsising, since losing
                                             the right half of an address
                                             defeats the point of showing it. -->
                                        <span
                                            v-if="comment.channel_metadata?.forwarded_by_user_uuid"
                                            class="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-semibold uppercase tracking-wide bg-accent-muted text-accent flex-shrink-0"
                                            :title="$t('ticket-comments-badge-forwarded-title')"
                                        >
                                            {{ $t('ticket-comments-badge-forwarded') }}
                                        </span>
                                        <span
                                            v-if="comment.from_address"
                                            class="text-xs text-tertiary break-all"
                                        >
                                            {{ comment.from_address }}
                                        </span>
                                        </div>
                                    </div><!-- /name+meta column -->
                                </div><!-- /header row (avatar + meta) -->
                                <!-- Body. Spans the full width of the
                                     content column so the email iframe
                                     inside isn't indented under the
                                     avatar. -->
                                <div class="min-w-0">
                                    <CommentContent
                                        v-if="hasRealContent(comment)"
                                        :content="comment.content"
                                        :content-format="comment.content_format"
                                        :render-kind="comment.render_kind"
                                        :new-content="comment.new_content"
                                        :quoted-content="comment.quoted_content"
                                        :has-raw-source="comment.has_raw_source"
                                        :comment-id="comment.id"
                                    />
                                    <p v-else-if="isAudioOnlyComment(comment)" class="text-primary text-sm">
                                        {{ getAudioDisplayName(comment.attachments?.[0]?.name ?? '') }}
                                    </p>
                                </div>
                            </div><!-- /desktop content column -->
                        </div>
                        <!-- Attachment previews section -->
                        <div
                            v-if="comment.attachments && comment.attachments.length > 0"
                            class="flex flex-col gap-2"
                        >
                            <template v-for="(attachment, index) in comment.attachments" :key="attachment.url">
                                <AttachmentPreview
                                    :attachment="attachment"
                                    :author="comment.user?.name || comment.user_uuid"
                                    :timestamp="formattedDate(comment.createdAt ?? comment.created_at)"
                                    :show-delete="!isAudioOnlyComment(comment)"
                                    :hide-header="isAudioOnlyComment(comment)"
                                    @delete="deleteAttachment(comment.id, index)"
                                />
                            </template>
                        </div>
                    </div>
                </div>

                <!--
                  Print-only comments layout. Two shapes depending on
                  the comment's source format:
                    - HTML emails get a block layout: header on its
                      own line, body in a sanitised `<div>` so its
                      paragraphs / lists / blockquotes render as block
                      elements and break naturally across pages.
                    - Plaintext / Markdown / legacy comments keep the
                      compact inline `Author (Date): Content` format,
                      which packs short native staff comments densely.
                  Sanitisation runs through the shared `sanitiseHtml`
                  helper so `<meta>`, `<style>`, `<script>` and
                  `<head>` (which inbound emails routinely ship) get
                  stripped — they were leaking to the page as visible
                  text when the markdown renderer mishandled them.
                -->
                <div
                    v-if="props.comments.length > 0"
                    class="hidden print:block print-comments-container"
                >
                    <div
                        v-for="comment in props.comments"
                        :key="'print-' + comment.id"
                        class="print-comment"
                        :class="{ 'print-comment--block': comment.content_format === 'html' && hasRealContent(comment) }"
                    >
                        <template v-if="comment.content_format === 'html' && hasRealContent(comment)">
                            <!-- Block layout: header above body so the
                                 body can flow across page breaks. -->
                            <div class="print-comment-header">
                                <span class="print-comment-author">{{ comment.user?.name || $t('ticket-comments-print-unknown-author') }}</span>
                                <span class="print-comment-date">{{ formattedDate(comment.createdAt ?? comment.created_at) }}</span>
                            </div>
                            <div class="print-email-body" v-html="sanitiseHtml(comment.content)" />
                            <div
                                v-if="comment.attachments && comment.attachments.length > 0"
                                class="print-attachments"
                            >
                                <span
                                    v-for="(attachment, idx) in comment.attachments"
                                    :key="attachment.id"
                                >{{ idx > 0 ? ', ' : '' }}[{{ attachment.name }}]</span>
                            </div>
                        </template>
                        <template v-else>
                            <!-- Inline format: "Author (Date): Content" -->
                            <span class="print-comment-author">{{ comment.user?.name || $t('ticket-comments-print-unknown-author') }}</span>
                            <span class="print-comment-date">({{ formattedDate(comment.createdAt ?? comment.created_at) }}):</span>
                            <span v-if="hasRealContent(comment)" class="print-comment-content">
                                <MarkdownRenderer :content="comment.content" />
                            </span>
                            <span v-else-if="isAudioOnlyComment(comment)" class="print-comment-audio">
                                [Voice: {{ getAudioDisplayName(comment.attachments?.[0]?.name ?? '') }}<template v-if="comment.attachments?.[0]?.transcription"> — "{{ comment.attachments[0].transcription }}"</template>]
                            </span>
                            <span
                                v-if="comment.attachments && comment.attachments.length > 0 && !isAudioOnlyComment(comment)"
                                class="print-attachments"
                            >
                                <span
                                    v-for="(attachment, idx) in comment.attachments"
                                    :key="attachment.id"
                                >{{ idx > 0 ? ', ' : ' ' }}[{{ attachment.name }}]</span>
                            </span>
                        </template>
                    </div>
                </div>
            </div>
        </template>
    </SectionCard>
</template>

<style scoped>
@media print {
    .print-comments-container {
        font-size: 9pt;
        line-height: 1.4;
    }

    .print-comment {
        margin-bottom: 4pt;
        /* Allow tall email comments to break across pages — keeping
           them whole stranded a half-empty page above the comment.
           `orphans` / `widows` keep the author + date intro glued
           to at least three lines of body content so a comment never
           leaves its header alone at the bottom of a page. */
        page-break-inside: auto;
        break-inside: auto;
        orphans: 3;
        widows: 3;
    }

    /* Block-layout HTML emails get a little more breathing room
       between entries so the page doesn't read as a single wall of
       prose. */
    .print-comment--block {
        margin-bottom: 10pt;
    }

    .print-comment-header {
        margin-bottom: 2pt;
    }

    .print-comment-author {
        font-weight: 600;
        color: #000;
    }

    .print-comment-date {
        color: #666;
        font-size: 8pt;
        margin-right: 4pt;
    }

    .print-comment-content {
        color: #333;
    }

    /* Block-layout email body. Paragraphs and other block elements
       render naturally so the browser can break them across pages —
       which is the whole reason an HTML email gets this branch
       rather than the inline-`<p>` treatment below.

       `:deep(meta)` etc. is defence-in-depth: `sanitiseHtml` already
       drops them via the allow-list, but if a future profile relaxes
       that, the `display: none` here keeps them off the page. */
    .print-email-body {
        color: #222;
        font-size: 9pt;
        line-height: 1.45;
    }

    .print-email-body :deep(p),
    .print-email-body :deep(div) {
        display: block;
        margin: 0 0 4pt 0;
    }

    .print-email-body :deep(blockquote) {
        margin: 4pt 0 4pt 12pt;
        padding-left: 8pt;
        border-left: 2px solid #ccc;
        color: #555;
    }

    .print-email-body :deep(img) {
        max-width: 100%;
        max-height: 4in;
        height: auto;
    }

    .print-email-body :deep(meta),
    .print-email-body :deep(link),
    .print-email-body :deep(style),
    .print-email-body :deep(script),
    .print-email-body :deep(head),
    .print-email-body :deep(title) {
        display: none !important;
    }

    .print-comment-content :deep(p) {
        display: inline;
        margin: 0;
    }

    .print-comment-content :deep(p + p)::before {
        content: " ";
    }

    .print-comment-content :deep(ul),
    .print-comment-content :deep(ol) {
        display: block;
        margin: 2pt 0 2pt 12pt;
        padding-left: 0;
    }

    .print-comment-content :deep(li) {
        margin-bottom: 1pt;
    }

    .print-comment-content :deep(code) {
        background: #f0f0f0;
        padding: 0 2pt;
        font-size: 8pt;
    }

    .print-comment-content :deep(pre) {
        display: block;
        background: #f5f5f5;
        border: 1px solid #ddd;
        padding: 3pt 5pt;
        margin: 3pt 0;
        font-size: 8pt;
    }

    .print-comment-audio {
        color: #555;
        font-style: italic;
    }

    .print-attachments {
        color: #666;
        font-size: 8pt;
    }
}
</style>

