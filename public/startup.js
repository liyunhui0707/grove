// Independent of React and the entry module. No paths, exception text,
// preferences or photos are read, stored, transmitted or cleared here.
(() => {
  const panel = document.getElementById('startup');
  const message = document.getElementById('startup-message');
  const help = document.getElementById('startup-help');
  const code = document.getElementById('startup-code');
  const retry = document.getElementById('startup-retry');
  let ready = false;
  const fail = (kind) => {
    if (ready) return;
    const codes = { module: 'MG-START-MODULE', render: 'MG-START-RENDER', wait: 'MG-START-WAIT' };
    if (!Object.hasOwn(codes, kind)) return;
    panel.hidden = false;
    message.textContent =
      kind === 'wait' ? '界面尚未就绪，请稍候或重试。' : '界面未能打开，可以重试。';
    code.textContent = codes[kind];
    help.hidden = false;
    retry.hidden = false;
  };
  const timer = setTimeout(() => fail('wait'), 15000);
  window.__momentStartup = Object.freeze({
    fail,
    ready() {
      if (ready) return;
      ready = true;
      clearTimeout(timer);
      panel.hidden = true;
      retry.hidden = true;
    },
    isReady: () => ready,
  });
  window.addEventListener(
    'error',
    (event) => {
      if (
        event.target instanceof HTMLScriptElement &&
        (event.target.hasAttribute('data-startup-entry') || event.target.type === 'module')
      )
        fail('module');
    },
    true,
  );
  retry.addEventListener('click', () => {
    // Once React commits, a reload could interrupt ratings or file operations.
    // Late errors/timeouts must never reopen this startup-only action.
    if (!ready) window.location.reload();
  });
})();
