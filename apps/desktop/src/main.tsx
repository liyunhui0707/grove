import React from 'react';
import ReactDOM from 'react-dom/client';
import { recordStartup, startupFailed, startupReady } from './startup';
import './styles.css';

recordStartup('frontend_entry');
function Ready({ children }: { children: React.ReactNode }) {
  React.useLayoutEffect(startupReady, []);
  return children;
}

void import('./App')
  .then(({ App }) => {
    ReactDOM.createRoot(document.getElementById('root')!, {
      onUncaughtError: () => startupFailed('render'),
    }).render(
      <React.StrictMode>
        <Ready>
          <App />
        </Ready>
      </React.StrictMode>,
    );
  })
  .catch(() => startupFailed('module'));
