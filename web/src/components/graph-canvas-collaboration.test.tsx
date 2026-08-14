import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { CanvasCollaborationWorld } from './graph-canvas-collaboration';

describe('CanvasCollaborationWorld', () => {
  it('renders collaborators without echoing the current actor', () => {
    const markup = renderToStaticMarkup(
      <CanvasCollaborationWorld
        comments={[]}
        hiddenActorId="local:current-tab"
        nodes={[]}
        presenceByActor={{
          self: {
            actor: { actorId: 'local:current-tab', displayName: 'Local user' },
            cursor: { x: 10, y: 20 },
          },
          legacySelf: {
            actor: { actorId: 'local', displayName: 'Legacy local user' },
            cursor: { x: 20, y: 30 },
          },
          remote: {
            actor: { actorId: 'remote', displayName: 'Collaborator' },
            cursor: { x: 30, y: 40 },
          },
        }}
      />,
    );

    expect(markup).not.toContain('Local user');
    expect(markup).not.toContain('Legacy local user');
    expect(markup).toContain('Collaborator');
    expect(markup.match(/collab-cursor/g)).toHaveLength(1);
    expect(markup).toContain('translate3d(38px, 48px, 0)');
    expect(markup).not.toContain('left:30px');
  });
});
