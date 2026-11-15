import { invoke, isTauri } from '@tauri-apps/api/core';

declare global {
  interface Window {
    __momentStartup?: {
      fail: (kind: 'module' | 'render' | 'wait') => void;
      ready: () => void;
      isReady: () => boolean;
    };
  }
}

let writes: Promise<unknown> = Promise.resolve();
export function recordStartup(stage: 'frontend_entry' | 'frontend_ready' | 'frontend_failed') {
  if (isTauri()) {
    // Diagnostic failures never prevent browsing and never log raw errors.
    writes = writes
      .catch(() => {})
      .then(() => invoke('record_startup_stage', { stage }))
      .catch(() => {});
  }
}

export function startupFailed(kind: 'module' | 'render') {
  if (window.__momentStartup?.isReady()) return;
  window.__momentStartup?.fail(kind);
  recordStartup('frontend_failed');
}

export function startupReady() {
  window.__momentStartup?.ready();
  // Also work if the independent guard script itself could not load.
  const panel = document.getElementById('startup');
  if (panel) panel.hidden = true;
  recordStartup('frontend_ready');
}
