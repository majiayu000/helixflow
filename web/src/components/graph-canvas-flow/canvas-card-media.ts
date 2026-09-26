import { addCreationAsset } from '../../creation-assets';
import {
  artifactsForNode,
  fetchArtifactBlob,
  fetchWorkspaceUploadContent,
  resolveGridSplitSource,
} from '../../grid-split';
import type { WorkbenchState } from '../../types';

type CardMediaInput = {
  node: WorkbenchState['graph']['nodes'][number];
  outputs: WorkbenchState['outputs'] | undefined;
  params: unknown;
  workspaceId: string;
};

export async function saveCardAsset(input: CardMediaInput): Promise<void> {
  const blob = await readCardBlob(input);
  const kind = blob.type.startsWith('video/')
    ? 'video'
    : blob.type.startsWith('audio/')
      ? 'audio'
      : 'image';
  await addCreationAsset({
    title: input.node.title || input.node.id,
    kind,
    file: blob,
  });
}

export async function downloadCardMedia(input: CardMediaInput): Promise<void> {
  const blob = await readCardBlob(input);
  const extension = blob.type.includes('video') ? 'mp4' : blob.type.includes('audio') ? 'mp3' : 'png';
  const objectUrl = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = objectUrl;
  link.download = `${input.node.title || 'media'}.${extension}`;
  link.click();
  URL.revokeObjectURL(objectUrl);
}

async function readCardBlob(input: CardMediaInput): Promise<Blob> {
  const source = resolveGridSplitSource({
    nodeType: input.node.nodeType,
    params: input.params,
    artifacts: artifactsForNode(input.outputs, input.node.id),
  });
  if (!source) throw new Error('没有可读取的文件');
  return source.kind === 'upload'
    ? fetchWorkspaceUploadContent(input.workspaceId, source.uploadId)
    : fetchArtifactBlob(source.artifactId);
}
