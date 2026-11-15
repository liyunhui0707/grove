import type { ReviewView } from './workspace';

type Checkpoint = { folderId: number | null; view: ReviewView };

// One writer per App. Retain the latest failed checkpoint for explicit retry;
// an older failure must never hide a newer queued position.
export function createWorkspaceWriter(persist: (value: Checkpoint) => Promise<void>) {
  let pending: Checkpoint | null = null;
  let revision = 0;
  let running = false;
  let tail: Promise<void> = Promise.resolve();
  const start = () => {
    running = true;
    tail = (async () => {
      try {
        while (pending) {
          const value = pending;
          const ticket = revision;
          try {
            await persist(value);
          } catch (error) {
            if (ticket === revision) throw error;
            continue;
          }
          if (ticket === revision) pending = null;
        }
      } finally {
        running = false;
      }
    })();
    return tail;
  };
  return {
    save(value: Checkpoint) {
      pending = { folderId: value.folderId, view: { ...value.view } };
      revision++;
      return running ? tail : start();
    },
    async flush(retry = false) {
      if (retry && pending && !running) void start().catch(() => {});
      for (;;) {
        const observed = tail;
        try {
          await observed;
        } catch (error) {
          if (observed === tail) throw error;
        }
        if (observed === tail) return;
      }
    },
  };
}
