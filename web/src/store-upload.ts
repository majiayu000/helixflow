import type { StoreApi } from 'zustand';
import { uploadWorkspaceImage } from './api';
import { appendChatMessages, appendSystemError, messageTime } from './store-model';
import type { WorkbenchStore } from './store-types';
import type { WorkspaceRequestScope } from './workspace-request-scope';
import { WorkspaceChangedError } from './workspace-action-guard';

/// Image upload action: posts the file to the workspace uploads endpoint and
/// appends a system chat message telling the user which storage_uri to wire
/// into input.image nodes. Kept out of store.ts to respect the file-size
/// budget; wired as `uploadImage: createUploadImageAction(set, get)`.
export function createUploadImageAction(
  set: StoreApi<WorkbenchStore>['setState'],
  get: StoreApi<WorkbenchStore>['getState'],
  requestScope: WorkspaceRequestScope,
): (file: File) => Promise<void> {
  return async (file) => {
    const state = get().state;
    if (!state) return;
    const workspaceId = state.workspace.id;
    const generation = requestScope.currentGeneration();
    if (!requestScope.isActive(generation, workspaceId)) {
      throw new WorkspaceChangedError('workspace changed before the image upload could start');
    }
    try {
      const uploaded = await uploadWorkspaceImage(workspaceId, file, requestScope.signal());
      if (!requestScope.isActive(generation, workspaceId)) {
        throw new WorkspaceChangedError('workspace changed while the image was uploading');
      }
      set((current) => ({
        state: current.state?.workspace.id === workspaceId
          ? appendChatMessages(current.state, [
              {
                id: `msg_system_${Date.now()}`,
                role: 'system',
                kind: 'text',
                text: `图片已上传：${uploaded.filename} → 在 input.image 节点的 storage_uri 参数填入 ${uploaded.storageUri}`,
                time: messageTime(),
              },
            ])
          : current.state,
      }));
    } catch (error) {
      if (
        error instanceof WorkspaceChangedError ||
        requestScope.signal()?.aborted ||
        !requestScope.isActive(generation, workspaceId)
      ) {
        throw new WorkspaceChangedError('workspace changed while the image was uploading');
      }
      const message = error instanceof Error ? error.message : 'image upload failed';
      set((current) => ({
        state: current.state?.workspace.id === workspaceId
          ? appendSystemError(current.state, message)
          : current.state,
      }));
      throw error;
    }
  };
}
