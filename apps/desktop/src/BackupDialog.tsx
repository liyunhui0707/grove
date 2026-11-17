import { dialogControls, TaskError } from './TaskFeedback';
import { useEffect, useRef, useState } from 'react';
import { Archive, Check, LoaderCircle, Plus, RotateCcw, ShieldCheck, X } from 'lucide-react';
import { bridge, native, type Restored, type Snapshot } from './bridge';

export function BackupDialog({
  onClose,
  onRestored,
}: {
  onClose: () => void;
  onRestored: (result: Restored) => void;
}) {
  const panel = useRef<HTMLElement>(null);
  const [items, setItems] = useState<Snapshot[]>([]);
  const [unavailable, setUnavailable] = useState(0);
  const [busy, setBusy] = useState(native);
  const [error, setError] = useState('');
  const [readAttempt, setReadAttempt] = useState(0);
  const [notice, setNotice] = useState('');
  const [confirm, setConfirm] = useState<Snapshot | null>(null);
  useEffect(() => {
    const element = panel.current;
    if (element && !element.contains(document.activeElement)) {
      (element.querySelector<HTMLButtonElement>('button:not(:disabled)') ?? element).focus();
    }
  }, [busy, confirm]);
  useEffect(() => {
    (
      panel.current?.querySelector<HTMLButtonElement>('button:not(:disabled)') ?? panel.current
    )?.focus();
    let cancelled = false;
    setBusy(native);
    setError('');
    if (native) {
      bridge
        .backups()
        .then((result) => {
          if (cancelled) return;
          setItems(result.items);
          setUnavailable(result.unavailable);
        })
        .catch((e) => {
          if (!cancelled) setError(String(e));
        })
        .finally(() => {
          if (!cancelled) setBusy(false);
        });
    }
    return () => {
      cancelled = true;
    };
  }, [readAttempt]);
  const create = async () => {
    if (busy || !native) return;
    setBusy(true);
    setError('');
    setNotice('');
    try {
      const item = await bridge.createBackup();
      setItems((current) => [item, ...current]);
      setNotice('资料库快照已保存');
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const restore = async () => {
    if (busy || !confirm || !native) return;
    setBusy(true);
    setError('');
    setNotice('');
    try {
      onRestored(await bridge.restoreBackup(confirm.id));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const date = (item: Snapshot) => new Date(item.created).toLocaleString();
  return (
    <div
      className="modal-backdrop"
      onClick={() => {
        if (!busy) onClose();
      }}
    >
      <section
        className="backup-dialog"
        ref={panel}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label="资料库备份"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === 'Escape' && !busy) {
            e.preventDefault();
            if (confirm) setConfirm(null);
            else onClose();
          }
          if (e.key === 'Tab') {
            const buttons = dialogControls(panel.current);
            const first = buttons[0],
              last = buttons.at(-1);
            if (!first) {
              e.preventDefault();
              panel.current?.focus();
            } else if (
              e.shiftKey &&
              (document.activeElement === first ||
                !buttons.includes(document.activeElement as HTMLElement))
            ) {
              e.preventDefault();
              last?.focus();
            } else if (
              !e.shiftKey &&
              (document.activeElement === last ||
                !buttons.includes(document.activeElement as HTMLElement))
            ) {
              e.preventDefault();
              first.focus();
            }
          }
        }}
      >
        <div className="dialog-heading">
          <Archive size={22} />
          <div>
            <h2>资料库备份</h2>
          </div>
          <button
            className="icon-button"
            aria-label="关闭资料库备份"
            disabled={busy}
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </div>
        <p className="backup-intro">保存评分、文件关联和待同步记录。照片原件保持不变。</p>
        <div className="backup-scope">
          <ShieldCheck size={17} />
          <p>
            快照保存在本机应用数据目录，包含文件关联和 XMP
            元数据，不含照片文件。它可用于资料库回退；请继续保留独立的媒体备份。
          </p>
        </div>
        {error && (
          <TaskError
            error={error}
            title="资料库备份未完成"
            action={
              <button
                className="button subtle"
                disabled={busy}
                onClick={() => setReadAttempt((n) => n + 1)}
              >
                重新读取列表
              </button>
            }
          />
        )}
        {notice && (
          <p className="backup-notice" role="status">
            <Check size={14} />
            {notice}
          </p>
        )}
        {!native ? (
          <div className="backup-empty">
            请在桌面 App 中创建和恢复快照。浏览器预览不使用本机资料库。
          </div>
        ) : confirm ? (
          <div className="restore-confirm">
            <span className="eyebrow">恢复备份</span>
            <h3>{date(confirm)}</h3>
            <p>
              {confirm.photos} 条照片记录 · 已评分 {confirm.reviewed} 条（含 {confirm.rejected}{' '}
              条淘汰候选）
            </p>
            <p>
              当前资料库将回到这一时刻。恢复前会先自动备份当前状态；如果备份失败，本次恢复会停止。恢复后需重新选择照片目录。已写出的
              XMP 不会回滚，需在评分同步中核对。
            </p>
            <div className="backup-actions">
              <button className="button subtle" disabled={busy} onClick={() => setConfirm(null)}>
                返回快照列表
              </button>
              <button className="button primary" disabled={busy} onClick={() => void restore()}>
                <RotateCcw size={15} />
                确认恢复资料库
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className="backup-list-heading">
              <h3>
                本机资料库备份 <small>{items.length}</small>
              </h3>
              <button className="button primary" disabled={busy} onClick={() => void create()}>
                <Plus size={15} />
                创建备份
              </button>
            </div>
            {unavailable > 0 && (
              <p className="backup-error">另有 {unavailable} 份备份未通过核验，已阻止恢复。</p>
            )}
            <div className="backup-list">
              {items.length ? (
                items.map((item) => (
                  <div className="backup-item" key={item.id}>
                    <Archive size={19} />
                    <div>
                      <strong>{date(item)}</strong>
                      <p>
                        {item.photos} 条记录 · 已评分 {item.reviewed} 条 ·{' '}
                        {(item.bytes / 1024).toFixed(0)} KB
                      </p>
                    </div>
                    <button
                      className="button subtle"
                      disabled={busy}
                      aria-label={`恢复 ${date(item)} ${item.id.slice(0, 8)}`}
                      onClick={() => {
                        setConfirm(item);
                        setNotice('');
                      }}
                    >
                      恢复…
                    </button>
                  </div>
                ))
              ) : (
                <div className="backup-empty">
                  {busy ? '正在读取快照…' : '还没有资料库备份。创建后可以恢复评分记录。'}
                </div>
              )}
            </div>
          </>
        )}
        {busy && (
          <p className="backup-busy" role="status">
            <LoaderCircle className="spin" size={14} />
            正在核验资料库，请稍候…
          </p>
        )}
      </section>
    </div>
  );
}
