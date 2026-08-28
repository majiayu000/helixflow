import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './app';
import '@xyflow/react/dist/style.css';
import './styles.css';
import './inspector.css';
import './connections.css';
import './node-library.css';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
