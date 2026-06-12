import React from 'react';
import ReactDOM from 'react-dom/client';
import './styles/styles.css';
import './styles/app.css';
import { App } from './App';
import { engine } from './engine/engine';

if (import.meta.env.DEV) {
  // Dev/E2E hooks for driving the engine/pipeline directly from the console.
  const w = window as unknown as Record<string, unknown>;
  w.__veEngine = engine;
  void import('./engine/mediaSession').then((m) => {
    w.__veMedia = m;
  });
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
