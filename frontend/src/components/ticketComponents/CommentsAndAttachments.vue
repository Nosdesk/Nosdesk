<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { ref } from "vue";
import UserAvatar from "@/components/UserAvatar.vue";
import VoiceRecorder from "@/components/ticketComponents/VoiceRecorder.vue";
import AttachmentPreview from "@/components/ticketComponents/AttachmentPreview.vue";
import SectionCard from "@/components/common/SectionCard.vue";
import SimpleEditor from "@/components/common/SimpleEditor.vue";
import MarkdownRenderer from "@/components/common/MarkdownRenderer.vue";
import uploadService from "@/services/uploadService";
import { convertToAuthenticatedPath } from '@/services/fileService';

interface UserInfo {
    uuid: string;
    name: string;
    avatar_url?: string | null;
    avatar_thumb?: string | null;
}

interface CommentWithAttachments {
    id: number;
    content: string;
    user_uuid: string;
    createdAt: string;
    attachments?: {
        id: number;
        url: string;
        name: string;
        comment_id: number;
        transcription?: string;
    }[];
    user?: UserInfo;
}

const props = defineProps<{
    comments: CommentWithAttachments[];
    currentUser: string;
    recentlyAddedCommentIds?: Set<number>;
}>();

const newCommentContent = ref<string>("");
const newAttachments = ref<File[]>([]); // Store File objects initially
const fileInputRef = ref<HTMLInputElement | null>(null);
const showRecordingInterface = ref(false);
const isDraggingFile = ref(false);
const conversionMessage = ref<string | null>(null);

const emit = defineEmits<{
    (
        e: "addComment",
        value: {
            content: string;
            user_uuid: string;
            files: File[];
        },
    ): void;
    (
        e: "deleteAttachment",
        value: { commentId: number; attachmentIndex: number },
    ): void;
    (e: "deleteComment", value: number): void;
}>();

const addComment = () => {
    if (!newCommentContent.value.trim() && newAttachments.value.length === 0)
        return;

    console.log("Emitting addComment event with data:", {
        content: newCommentContent.value,
        user_uuid: props.currentUser,
        files: newAttachments.value,
    });

    // Emit the comment with raw files - parent will handle upload
    emit("addComment", {
        content: newCommentContent.value,
        user_uuid: props.currentUser,
        files: newAttachments.value,
    });

    // Reset form
    newCommentContent.value = "";
    newAttachments.value = [];

    console.log("Comment submission complete, form reset");
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

        newAttachments.value.push(...processedFiles, ...audioFiles);
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
    const fileName = `Voice Note ${formatDate(new Date(), "MMM d, yyyy")}.webm`;
    const audioFile = new File([recording.blob], fileName, {
        type: recording.blob.type,
    }) as File & { _transcription?: string };

    if (recording.transcription) {
        (audioFile as any)._transcription = recording.transcription;
        console.log('[CommentsAndAttachments] Attached transcription to file');
    }

    newAttachments.value.push(audioFile);
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


const formattedDate = (dateString: string): string => {
    const date = new Date(dateString);
    return formatDate(dateString, "MMM d, yyyy");
};

// Check if comment has real text content (not just placeholder)
const hasRealContent = (comment: CommentWithAttachments): boolean => {
    const content = comment.content?.trim() || '';
    return content !== '' && content.toLowerCase() !== 'attachment added';
};

// Check if comment is audio-only (no text, single audio attachment)
const isAudioOnlyComment = (comment: CommentWithAttachments): boolean => {
    if (hasRealContent(comment)) return false;
    if (!comment.attachments || comment.attachments.length !== 1) return false;
    const name = comment.attachments[0].name?.toLowerCase() || '';
    const audioExtensions = ['.mp3', '.wav', '.ogg', '.m4a', '.webm', '.aac'];
    return audioExtensions.some(ext => name.endsWith(ext)) || name.includes('voice note');
};

// Get display name for audio - "Voice Message" for voice notes
const getAudioDisplayName = (filename: string): string => {
    if (!filename) return 'Audio';
    const lower = filename.toLowerCase();
    if (lower.startsWith('voice note') || lower.startsWith('voicenote')) {
        return 'Voice Message';
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

    newAttachments.value.push(...processedFiles, ...audioFiles);
};
</script>

<template>
    <SectionCard>
        <template #title>Comments and Attachments</template>

        <template #default>
            <div class="flex flex-col gap-3">
                <!-- Conversion Status Message (hidden on print) -->
                <div
                    v-if="conversionMessage"
                    class="print:hidden bg-accent/20 border border-accent/50 text-accent px-4 py-2 rounded-lg text-sm flex items-center gap-2"
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

                <!-- Add New Comment Form (hidden on print) -->
                <div
                    class="print:hidden bg-surface rounded-lg relative"
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
                            Drop files here
                        </div>
                    </div>

                    <form
                        @submit.prevent="addComment"
                        class="flex flex-col gap-2"
                    >
                        <SimpleEditor
                            v-model="newCommentContent"
                            placeholder="Add a new comment..."
                            min-height="60px"
                            max-height="200px"
                            @submit="addComment"
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
                                @delete="newAttachments.splice(index, 1)"
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

                        <div class="flex gap-2">
                            <button
                                type="submit"
                                class="flex-1 bg-accent text-white h-10 px-4 rounded-md hover:opacity-90 transition-colors text-sm font-medium"
                            >
                                Add
                            </button>
                            <!-- Voice Recording Button -->
                            <button
                                type="button"
                                @click="startVoiceRecording"
                                class="h-10 px-3 bg-surface-alt border border-default text-secondary rounded-md hover:bg-surface-hover hover:text-primary transition-colors flex items-center justify-center gap-2"
                                :class="{ 'text-error': showRecordingInterface }"
                                aria-label="Record voice note"
                                title="Record voice note"
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
                                class="h-10 px-3 bg-surface-alt border border-default text-secondary rounded-md hover:bg-surface-hover hover:text-primary transition-colors flex items-center justify-center gap-2"
                                aria-label="Upload file"
                                title="Upload file"
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
                        </div>
                    </form>
                </div>

                <!-- List of Comments -->
                <div
                    v-if="props.comments.length > 0"
                    class="flex flex-col gap-3"
                >
                    <div
                        v-for="comment in props.comments"
                        :key="comment.id"
                        class="flex flex-col gap-2 sm:gap-3 p-2 sm:p-3 rounded-lg border transition-all duration-300"
                        :class="[
                            props.recentlyAddedCommentIds?.has(comment.id)
                                ? 'bg-accent/20 border-accent/50 animate-pulse'
                                : 'bg-surface-alt border-subtle',
                        ]"
                    >
                        <!-- Mobile: Compact header with avatar, name, date, and actions inline -->
                        <!-- Desktop: Avatar on left, content beside it -->
                        <div class="flex flex-col sm:flex-row gap-2">
                            <!-- Header row: avatar, name/date, actions -->
                            <div class="flex items-center gap-2 sm:hidden">
                                <UserAvatar
                                    :name="comment.user?.uuid || comment.user_uuid"
                                    :userName="comment.user?.name"
                                    :avatar="comment.user?.avatar_thumb || comment.user?.avatar_url"
                                    :showName="false"
                                    size="sm"
                                    class="flex-shrink-0"
                                />
                                <div class="flex-1 min-w-0">
                                    <span class="text-sm text-primary font-medium truncate block">
                                        {{ comment.user?.name || comment.user_uuid }}
                                    </span>
                                    <span class="text-xs text-tertiary">
                                        {{ formattedDate(comment.createdAt) }}
                                    </span>
                                </div>
                                <!-- Mobile action buttons (hidden on print) -->
                                <div class="print:hidden flex items-center gap-1 flex-shrink-0">
                                    <a
                                        v-if="isAudioOnlyComment(comment)"
                                        :href="convertToAuthenticatedPath(comment.attachments[0].url)"
                                        :download="comment.attachments[0].name"
                                        target="_blank"
                                        class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
                                        title="Download"
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
                                        :title="isAudioOnlyComment(comment) ? 'Delete voice message' : 'Delete comment'"
                                    >
                                        <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
                                            <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
                                        </svg>
                                    </button>
                                </div>
                            </div>

                            <!-- Mobile: Full-width content below header -->
                            <div class="sm:hidden w-full">
                                <MarkdownRenderer
                                    v-if="hasRealContent(comment)"
                                    :content="comment.content"
                                    class="text-primary"
                                />
                                <p v-else-if="isAudioOnlyComment(comment)" class="text-primary text-sm">
                                    {{ getAudioDisplayName(comment.attachments[0].name) }}
                                </p>
                            </div>

                            <!-- Desktop: Header row with avatar, name, date, then content below -->
                            <div class="hidden sm:flex sm:flex-col gap-2 flex-1 min-w-0">
                                <!-- Header: avatar, name/date, actions -->
                                <div class="flex items-center gap-2">
                                    <UserAvatar
                                        :name="comment.user?.uuid || comment.user_uuid"
                                        :userName="comment.user?.name"
                                        :avatar="comment.user?.avatar_thumb || comment.user?.avatar_url"
                                        :showName="false"
                                        size="sm"
                                        class="flex-shrink-0"
                                    />
                                    <span class="text-sm text-primary font-medium">
                                        {{ comment.user?.name || comment.user_uuid }}
                                    </span>
                                    <span class="text-xs text-tertiary">
                                        {{ formattedDate(comment.createdAt) }}
                                    </span>
                                </div>
                                <!-- Content below header -->
                                <div class="min-w-0">
                                    <MarkdownRenderer
                                        v-if="hasRealContent(comment)"
                                        :content="comment.content"
                                        class="text-primary"
                                    />
                                    <p v-else-if="isAudioOnlyComment(comment)" class="text-primary text-sm">
                                        {{ getAudioDisplayName(comment.attachments[0].name) }}
                                    </p>
                                </div>
                            </div>
                            <!-- Desktop action buttons (hidden on print) -->
                            <div class="print:hidden hidden sm:flex items-center gap-1 flex-shrink-0 self-start">
                                <a
                                    v-if="isAudioOnlyComment(comment)"
                                    :href="convertToAuthenticatedPath(comment.attachments[0].url)"
                                    :download="comment.attachments[0].name"
                                    target="_blank"
                                    class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
                                    title="Download"
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
                                    :title="isAudioOnlyComment(comment) ? 'Delete voice message' : 'Delete comment'"
                                >
                                    <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
                                        <path fill-rule="evenodd" d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z" clip-rule="evenodd" />
                                    </svg>
                                </button>
                            </div>
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
                                    :timestamp="formattedDate(comment.createdAt)"
                                    :show-delete="!isAudioOnlyComment(comment)"
                                    :hide-header="isAudioOnlyComment(comment)"
                                    @delete="deleteAttachment(comment.id, index)"
                                />
                            </template>
                        </div>
                    </div>
                </div>
            </div>
        </template>
    </SectionCard>
</template>

