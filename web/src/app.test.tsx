import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { App } from './app';

describe('App', () => {
  it('renders the Helixflow scaffold shell', () => {
    const markup = renderToStaticMarkup(<App />);

    expect(markup).toContain('Helixflow');
    expect(markup).toContain('AI workflow orchestration');
    expect(markup).toContain('graph proposals');
  });
});
