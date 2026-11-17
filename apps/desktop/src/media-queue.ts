// Visible requests take queued slots before speculative work. One speculative
// request at a time shares the existing four-request limit; it adds no worker.
type Priority = 'visible' | 'prefetch';
type Job = {
  signal: AbortSignal;
  run: () => Promise<void>;
  priority: Priority;
  done: () => void;
  abort: () => void;
};
export function createMediaQueue(limit = 4) {
  const queue: Job[] = [];
  let running = 0;
  let prefetching = 0;
  function drain() {
    while (running < limit) {
      let index = queue.findIndex((job) => job.priority === 'visible');
      if (index < 0 && !prefetching) index = queue.findIndex((job) => job.priority === 'prefetch');
      if (index < 0) return;
      const [job] = queue.splice(index, 1);
      if (job.signal.aborted) {
        job.done();
        continue;
      }
      running++;
      if (job.priority === 'prefetch') prefetching++;
      void Promise.resolve()
        .then(job.run)
        .catch(() => {})
        .finally(() => {
          running--;
          if (job.priority === 'prefetch') prefetching--;
          job.done();
          drain();
        });
    }
  }
  return (signal: AbortSignal, run: () => Promise<void>, priority: Priority = 'visible') =>
    new Promise<void>((resolve) => {
      if (signal.aborted) {
        resolve();
        return;
      }
      const job: Job = {
        signal,
        run,
        priority,
        abort: () => {},
        done: () => {
          signal.removeEventListener('abort', job.abort);
          resolve();
        },
      };
      job.abort = () => {
        const index = queue.indexOf(job);
        if (index >= 0) {
          queue.splice(index, 1);
          job.done();
        }
      };
      signal.addEventListener('abort', job.abort, { once: true });
      queue.push(job);
      drain();
    });
}
