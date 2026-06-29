import { Icon } from '../icons';
import type { WorkbenchState } from '../types';

type ArtifactOutput = WorkbenchState['outputs'][number];

type ArtifactStageProps = {
  outputs: WorkbenchState['outputs'];
};

export function ArtifactStage({ outputs }: ArtifactStageProps) {
  const artifact = selectedPreviewArtifact(outputs);
  if (!artifact?.preview) return null;

  return (
    <section className="artifact-stage">
      <div className="artifact-head">
        <div className="artifact-title">
          <Icon n={artifactIcon(artifact.kind)} s={14} />
          <span>{artifact.title}</span>
        </div>
        <div className="artifact-meta">
          {artifact.kind}
          {artifact.mime ? ` · ${artifact.mime}` : ''}
        </div>
      </div>
      <div className="artifact-preview">
        {artifact.preview.kind === 'html' ? (
          <iframe
            className="artifact-frame"
            sandbox="allow-scripts"
            srcDoc={artifact.preview.content}
            title={artifact.title}
          />
        ) : (
          <pre className="artifact-text-preview">{artifact.preview.content}</pre>
        )}
      </div>
    </section>
  );
}

export function hasPreviewArtifact(outputs: WorkbenchState['outputs']): boolean {
  return Boolean(selectedPreviewArtifact(outputs));
}

function selectedPreviewArtifact(outputs: WorkbenchState['outputs']): ArtifactOutput | undefined {
  return outputs.find((output) => output.selected && output.preview)
    ?? outputs.find((output) => output.preview);
}

function artifactIcon(kind: ArtifactOutput['kind']): 'export' | 'image' | 'layers' | 'play' {
  if (kind === 'html' || kind === 'markdown' || kind === 'text') return 'export';
  if (kind === 'image') return 'image';
  if (kind === 'video') return 'play';
  return 'layers';
}
