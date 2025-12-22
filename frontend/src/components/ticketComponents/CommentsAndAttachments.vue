<script setup lang="ts">
import { formatDate, formatDateTime } from '@/utils/dateUtils';
import { ref } from "vue";
import UserAvatar from "@/components/UserAvatar.vue";
import VoiceRecorder from "@/components/ticketComponents/VoiceRecorder.vue";
import AttachmentPreview from "@/components/ticketComponents/AttachmentPreview.vue";
import AudioPreview from "@/components/ticketComponents/AudioPreview.vue";
import SectionCard from "@/components/common/SectionCard.vue";
import uploadService from "@/services/uploadService";

interface UserInfo {
    uuid: string;
    name: string;
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
const showPreviewInterface = ref(false);
const currentRecording = ref<{ blob: Blob; duration: number } | null>(null);
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
        console.log(
            "Files selected:",
            files.map((f) => `${f.name} (${f.type})`),
        );

        // Handle audio files separately
        const audioFiles = files.filter((file) =>
            file.type.startsWith("audio/"),
        );
        const otherFiles = files.filter(
            (file) => !file.type.startsWith("audio/"),
        );

        // Process audio files
        if (audioFiles.length > 0) {
            const audioFile = audioFiles[0]; // Take the first audio file
            currentRecording.value = { blob: audioFile, duration: 0 };
            showPreviewInterface.value = true;
        }

        // Process other files - convert HEIC images if needed
        const processedFiles = await processFiles(otherFiles);
        newAttachments.value.push(...processedFiles);
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
}) => {
    currentRecording.value = recording;
    showRecordingInterface.value = false;
    showPreviewInterface.value = true;
};

const handleRecordingCancel = () => {
    showRecordingInterface.value = false;
    currentRecording.value = null;
};

const handleAudioSubmit = (data: { blob: Blob; name: string }) => {
    // Convert Blob to File
    console.log("Handling audio submission:", data);
    const audioFile = new File([data.blob], data.name, {
        type: data.blob.type,
    });
    console.log("Created audio file:", audioFile);
    newAttachments.value.push(audioFile);
    showPreviewInterface.value = false;
    currentRecording.value = null;
};

const confirmRecording = () => {
    if (currentRecording.value) {
        const fileName = `Voice Note ${new Date().toLocaleTimeString()}.webm`;
        console.log("Confirming recording, creating file:", fileName);
        const audioFile = new File([currentRecording.value.blob], fileName, {
            type: currentRecording.value.blob.type,
        });
        console.log("Created audio file from recording:", audioFile);
        newAttachments.value.push(audioFile);
    }
    showPreviewInterface.value = false;
    currentRecording.value = null;
};

const reRecord = () => {
    showPreviewInterface.value = false;
    currentRecording.value = null;
    startVoiceRecording();
};

const cancelPreview = () => {
    showPreviewInterface.value = false;
    currentRecording.value = null;
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
    console.log(
        "Files dropped:",
        files.map((f) => `${f.name} (${f.type})`),
    );

    // Handle audio files separately
    const audioFiles = files.filter((file) => file.type.startsWith("audio/"));
    const otherFiles = files.filter((file) => !file.type.startsWith("audio/"));

    // Process audio files
    if (audioFiles.length > 0) {
        const audioFile = audioFiles[0]; // Take the first audio file
        currentRecording.value = { blob: audioFile, duration: 0 };
        showPreviewInterface.value = true;
    }

    // Process other files - convert HEIC images if needed
    const processedFiles = await processFiles(otherFiles);
    newAttachments.value.push(...processedFiles);
};
</script>

<template>
    <SectionCard>
        <template #title>Comments and Attachments</template>

        <template #default>
            <div class="flex flex-col gap-3">
                <!-- Conversion Status Message -->
                <div
                    v-if="conversionMessage"
                    class="bg-accent/20 border border-accent/50 text-accent px-4 py-2 rounded-lg text-sm flex items-center gap-2"
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

                <!-- Add New Comment Form -->
                <div
                    class="bg-surface rounded-lg relative"
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
                        <div class="relative">
                            <textarea
                                v-model="newCommentContent"
                                class="w-full bg-surface-alt text-primary border border-default rounded-md p-3 placeholder-tertiary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent resize-none"
                                placeholder="Add a new comment..."
                                rows="3"
                            ></textarea>
                        </div>

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

                        <!-- Voice Recorder and Preview Components -->
                        <VoiceRecorder
                            v-if="showRecordingInterface"
                            @recording-complete="handleRecordingComplete"
                            @cancel="handleRecordingCancel"
                        />

                        <AudioPreview
                            v-if="showPreviewInterface && currentRecording"
                            :blob="currentRecording.blob"
                            :author="props.currentUser"
                            :timestamp="formattedDate(new Date().toISOString())"
                            :show-recording-controls="showRecordingInterface"
                            @confirm="confirmRecording"
                            @submit="handleAudioSubmit"
                            @re-record="reRecord"
                            @cancel="cancelPreview"
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
                        class="flex flex-col gap-3 p-3 rounded-lg border transition-all duration-300"
                        :class="[
                            props.recentlyAddedCommentIds?.has(comment.id)
                                ? 'bg-accent/20 border-accent/50 animate-pulse'
                                : 'bg-surface-alt border-subtle',
                        ]"
                    >
                        <div class="flex flex-row gap-2 justify-between">
                            <div class="flex gap-2 justify-center items-center">
                                <UserAvatar
                                    :name="comment.user?.uuid || comment.user_uuid"
                                    :userName="comment.user?.name"
                                    :showName="false"
                                    size="md"
                                />
                                <div class="flex flex-col flex-grow">
                                    <p class="text-primary">
                                        {{ comment.content }}
                                    </p>
                                    <small class="text-secondary"
                                        >{{
                                            comment.user?.name ||
                                            comment.user_uuid
                                        }}
                                        -
                                        {{
                                            formattedDate(comment.createdAt)
                                        }}</small
                                    >
                                </div>
                            </div>
                            <button
                                type="button"
                                @click="deleteComment(comment.id)"
                                class="p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded-md transition-colors"
                                title="Delete comment"
                            >
                                <svg
                                    class="w-4 h-4"
                                    viewBox="0 0 20 20"
                                    fill="currentColor"
                                >
                                    <path
                                        fill-rule="evenodd"
                                        d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z"
                                        clip-rule="evenodd"
                                    />
                                </svg>
                            </button>
                        </div>
                        <div
                            v-if="
                                comment.attachments &&
                                comment.attachments.length > 0
                            "
                            class="flex items-center justify-center flex-wrap gap-2 bg-surface-alt rounded-lg p-2"
                        >
                            <AttachmentPreview
                                v-for="(
                                    attachment, index
                                ) in comment.attachments"
                                :key="attachment.url"
                                :attachment="attachment"
                                :author="
                                    comment.user?.name || comment.user_uuid
                                "
                                :timestamp="formattedDate(comment.createdAt)"
                                :show-delete="true"
                                @delete="deleteAttachment(comment.id, index)"
                            />
                        </div>
                    </div>
                </div>
            </div>
        </template>
    </SectionCard>
</template>

