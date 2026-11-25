import { useRef, useState, type ReactNode } from 'react';
import { bridge } from './bridge';

export function dialogControls(panel: HTMLElement | null): HTMLElement[] {
  return Array.from(
    panel?.querySelectorAll<HTMLElement>(
      'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), summary, a[href]',
    ) ?? [],
  ).filter(
    (el) =>
      el.tabIndex >= 0 &&
      el.getClientRects().length > 0 &&
      !(el.tagName !== 'SUMMARY' && el.closest('details:not([open])')),
  );
}

/** Keep actionable copy separate from backend diagnostics and personal paths. */
export function TaskError({
  error,
  title = '操作未完成',
  action,
  hint,
}: {
  error: string;
  title?: string;
  action?: ReactNode;
  hint?: string;
}) {
  if (!error) return null;
  return (
    <div className="task-error" role="alert">
      <strong>{title}</strong>
      <p>
        {hint ||
          (/请选择当前照片文件夹之外的导出位置/.test(error)
            ? '导出位置位于当前照片文件夹内。请选择与原片文件夹同级的输出文件夹。'
            : /空间|space|disk full/i.test(error)
              ? '目标磁盘空间不足。请选择空间充足的位置后重试。'
              : /冲突|变化|占用|identity|conflict/i.test(error)
                ? '文件或位置已变化。请核对相关文件，再重新准备清单；不会静默覆盖。'
                : /权限|离线|目录|EACCES|denied|permission/i.test(error)
                  ? '请确认文件夹在线且可以访问，再重试或重新选择位置。'
                  : '操作未能完成。请重新核对后重试；展开详情可查看具体原因。')}
      </p>
      {action}
      <details>
        <summary>详细信息</summary>
        <p className="technical-detail">{error}</p>
      </details>
    </div>
  );
}

export function DirectoryPath({ label, path }: { label: string; path: string }) {
  return (
    <div className="directory-path">
      <span>{label}</span>
      <strong>{path.split(/[\\/]/).filter(Boolean).at(-1) || path}</strong>
      <details>
        <summary>完整路径</summary>
        <code>{path}</code>
        <CopyPath value={path} />
      </details>
    </div>
  );
}

export function CopyPath({ value }: { value: string }) {
  const [notice, setNotice] = useState('');
  return (
    <>
      <button
        className="button subtle"
        onClick={() => {
          void navigator.clipboard?.writeText(value).then(
            () => setNotice('路径已复制'),
            () => setNotice('请选中上方路径，按 ⌘C 复制'),
          );
          if (!navigator.clipboard) setNotice('请选中上方路径，按 ⌘C 复制');
        }}
      >
        复制路径
      </button>
      {notice && <span role="status">{notice}</span>}
    </>
  );
}

export function exportHistoryLabel(path: string, kind: string, count: number) {
  const time = path
    .split(/[\\/]/)
    .at(-1)
    ?.match(/^(?:Grove|Moment-Grove)-(?:Watermark-)?(\d{10})-/)?.[1];
  return `${kind === 'watermark' ? '水印照片' : '原格式副本'} · ${count} 张${time ? ` · ${new Date(Number(time) * 1000).toLocaleString('zh-CN')}` : ''}`;
}

export function ExportTabs({
  watermark,
  disabled,
  onSwitch,
}: {
  watermark?: boolean;
  disabled?: boolean;
  onSwitch: () => void;
}) {
  const tabs = useRef<HTMLDivElement>(null);
  return (
    <div
      className="export-tabs"
      role="tablist"
      aria-label="导出方式"
      ref={tabs}
      onKeyDown={(e) => {
        if (disabled || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
        e.preventDefault();
        e.stopPropagation();
        const next = e.key === 'ArrowRight' || e.key === 'End';
        if (next !== Boolean(watermark)) onSwitch();
        else tabs.current?.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus();
      }}
    >
      <button
        role="tab"
        aria-selected={!watermark}
        tabIndex={watermark ? -1 : 0}
        disabled={disabled}
        onClick={watermark ? onSwitch : undefined}
      >
        原格式副本
      </button>
      <button
        role="tab"
        aria-selected={Boolean(watermark)}
        tabIndex={watermark ? 0 : -1}
        disabled={disabled}
        onClick={watermark ? undefined : onSwitch}
      >
        日期城市水印
      </button>
    </div>
  );
}

export function ExportCompletion({
  folderId,
  id,
  count,
  onClose,
}: {
  folderId: number;
  id: string;
  count: number;
  onClose: () => void;
}) {
  const [error, setError] = useState('');
  const [opening, setOpening] = useState(false);
  return (
    <div className="export-completion">
      <TaskError error={error} title="无法打开导出文件夹" />
      <span role="status">已导出并核验 {count} 张照片</span>
      <button
        className="button subtle"
        disabled={opening}
        onClick={() => {
          setOpening(true);
          setError('');
          void bridge
            .openExportFolder(folderId, id)
            .catch((e) => setError(String(e)))
            .finally(() => setOpening(false));
        }}
      >
        打开导出文件夹
      </button>
      <button className="button primary" onClick={onClose}>
        完成
      </button>
    </div>
  );
}

export function ToolsMenu({ children }: { children: ReactNode }) {
  const root = useRef<HTMLDetailsElement>(null);
  return (
    <details
      className="tools-menu"
      ref={root}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) e.currentTarget.open = false;
      }}
      onKeyDown={(e) => {
        if (e.key === 'Escape' && e.currentTarget.open) {
          e.preventDefault();
          e.stopPropagation();
          e.currentTarget.open = false;
          e.currentTarget.querySelector('summary')?.focus();
        }
      }}
    >
      <summary role="button" aria-label="资料库工具">
        资料库工具
      </summary>
      <div
        className="tools-menu-items"
        onClick={(e) => {
          if ((e.target as HTMLElement).closest('button') && root.current)
            root.current.open = false;
        }}
      >
        {children}
      </div>
    </details>
  );
}
