import { useEffect, useState } from 'react';

import { fetchArtifactText } from '../api';
import { Icon } from '../icons';
import type { WorkbenchState } from '../types';

type ArtifactOutput = WorkbenchState['outputs'][number];

type TextContentState =
  | { status: 'loading' }
  | { status: 'loaded'; text: string }
  | { status: 'error'; message: string };

function TextArtifactPreview({ artifact }: { artifact: ArtifactOutput }) {
  const [content, setContent] = useState<TextContentState>({ status: 'loading' });

  useEffect(() => {
    let cancelled = false;
    setContent({ status: 'loading' });
    fetchArtifactText(artifact.id)
      .then((text) => {
        if (!cancelled) setContent({ status: 'loaded', text });
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setContent({
            status: 'error',
            message: error instanceof Error ? error.message : 'artifact content request failed',
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [artifact.id]);

  if (content.status === 'loading') {
    return <pre className="artifact-text-preview">Loading artifact content…</pre>;
  }
  if (content.status === 'error') {
    return (
      <pre className="artifact-text-preview artifact-text-error">
        Failed to load artifact content: {content.message}
      </pre>
    );
  }
  return <pre className="artifact-text-preview">{content.text}</pre>;
}

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
            sandbox=""
            srcDoc={artifact.preview.content}
            title={artifact.title}
          />
        ) : artifact.preview.kind === 'image' ? (
          <img
            alt={artifact.title}
            className="artifact-media"
            src={artifact.preview.content}
            title={artifact.title}
          />
        ) : artifact.preview.kind === 'video' ? (
          <video
            className="artifact-media"
            controls
            src={artifact.preview.content}
            title={artifact.title}
          />
        ) : (
          <TextArtifactPreview artifact={artifact} />
        )}
      </div>
    </section>
  );
}

export function hasPreviewArtifact(outputs: WorkbenchState['outputs']): boolean {
  return Boolean(selectedPreviewArtifact(outputs));
}

function selectedPreviewArtifact(outputs: WorkbenchState['outputs']): ArtifactOutput | undefined {
  return outputs.find((output) => output.selected && output.preview);
}

function artifactIcon(kind: ArtifactOutput['kind']): 'export' | 'image' | 'layers' | 'play' {
  if (kind === 'html' || kind === 'markdown' || kind === 'text') return 'export';
  if (kind === 'image') return 'image';
  if (kind === 'video') return 'play';
  return 'layers';
}
