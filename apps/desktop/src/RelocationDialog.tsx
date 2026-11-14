import { useEffect, useRef, useState } from 'react';
import { ArrowRight, FolderOpen, Link2, LoaderCircle, ShieldCheck, X } from 'lucide-react';
import {
  bridge,
  native,
  type Relocated,
  type RelocationEntry,
  type RelocationReview,
} from './bridge';

export function RelocationDialog({
  onClose,
  onRelinked,
}: {
  onClose: () => void;
  onRelinked: (result: Relocated) => void;
}) {
  const panel = useRef<HTMLElement>(null);
  const [items, setItems] = useState<RelocationEntry[]>([]);
  const [busy, setBusy] = useState(native);
  const [error, setError] = useState('');
  const [review, setReview] = useState<RelocationReview | null>(null);
  useEffect(() => {
    let cancelled = false;
    if (native)
      void bridge
        .relocationFolders()
        .then((result) => {
          if (!cancelled) setItems(result);
        })
        .catch((e) => {
          if (!cancelled) setError(String(e));
        })
        .finally(() => {
          if (!cancelled) setBusy(false);
        });
    return () => {
      cancelled = true;
    };
  }, []);
  useEffect(() => {
    const element = panel.current;
    if (element && !element.contains(document.activeElement))
      (element.querySelector<HTMLButtonElement>('button:not(:disabled)') ?? element).focus();
  }, [busy, review]);
  const choose = async (id: number) => {
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      setReview(await bridge.chooseRelocation(id));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const confirm = async () => {
    if (busy || !review) return;
    setBusy(true);
    setError('');
    try {
      onRelinked(await bridge.confirmRelocation(review.token));
    } catch (e) {
      setError(String(e));
      setReview(null);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div
      className="modal-backdrop"
      onClick={() => {
        if (!busy) onClose();
      }}
    >
      <section
        className="backup-dialog relocation-dialog"
        ref={panel}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label="重新关联目录"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === 'Escape' && !busy) {
            e.preventDefault();
            if (review) setReview(null);
            else onClose();
          }
          if (e.key === 'Tab') {
            const buttons = Array.from(
              panel.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [],
            );
            const first = buttons[0],
              last = buttons.at(-1),
              active = document.activeElement as HTMLButtonElement;
            if (!first) {
              e.preventDefault();
              panel.current?.focus();
            } else if (e.shiftKey && (active === first || !buttons.includes(active))) {
              e.preventDefault();
              last?.focus();
            } else if (!e.shiftKey && (active === last || !buttons.includes(active))) {
              e.preventDefault();
              first.focus();
            }
          }
        }}
      >
        <div className="dialog-heading">
          <Link2 size={22} />
          <div>
            <span className="eyebrow">SAME MEMORIES, A NEW PLACE</span>
            <h2>让挑选，跟上照片。</h2>
          </div>
          <button
            className="icon-button"
            aria-label="关闭重新关联"
            disabled={busy}
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </div>
        <p className="dialog-intro">整个文件夹已换位置？核对新位置，让评分与待同步任务继续保留。</p>
        <div className="backup-scope">
          <ShieldCheck size={17} />
          <p>
            只更新资料库中的位置，不搬动或改写照片。确认前自动创建快照；跨磁盘副本、缺失文件和身份不符会暂停关联。
          </p>
        </div>
        {error && (
          <p className="backup-error" role="alert">
            {error}
          </p>
        )}
        {!native ? (
          <div className="backup-empty">
            请在桌面 App 中重新关联。浏览器预览不访问本机目录或修改资料库。
          </div>
        ) : review ? (
          <div className="restore-confirm">
            <span className="eyebrow">RECONNECT THIS FOLDER</span>
            <div className="relocation-route">
              <strong>{review.oldName}</strong>
              <ArrowRight size={18} />
              <strong>{review.newName}</strong>
            </div>
            <p>
              已核对 {review.photos} 张照片 · 保留 {review.pending} 项待同步评分
            </p>
            <p>
              确认时会再次核对文件身份。七态评分和撤销记录保留；XMP 仍由你在评分同步中明确触发。
            </p>
            <div className="backup-actions">
              <button className="button subtle" disabled={busy} onClick={() => setReview(null)}>
                返回目录列表
              </button>
              <button className="button primary" disabled={busy} onClick={() => void confirm()}>
                <Link2 size={15} />
                备份并确认关联
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className="backup-list-heading">
              <h3>
                已记录目录 <small>{items.length}</small>
              </h3>
            </div>
            <div className="backup-list">
              {items.length ? (
                items.map((item) => (
                  <div className="backup-item" key={item.id}>
                    <FolderOpen size={19} />
                    <div>
                      <strong>{item.name}</strong>
                      <p>
                        {item.photos} 张照片 · {item.pending} 项待同步
                      </p>
                    </div>
                    <button
                      className="button subtle"
                      disabled={busy}
                      onClick={() => void choose(item.id)}
                      aria-label={`为 ${item.name} 选择新位置`}
                    >
                      选择新位置…
                    </button>
                  </div>
                ))
              ) : (
                <div className="backup-empty">
                  {busy ? '正在读取目录记录…' : '还没有目录记录。先打开一个测试照片文件夹。'}
                </div>
              )}
            </div>
            <p className="relocation-hint">
              用于同一磁盘上的整体移动或改名。若已复制到其他电脑，先保留原资料库；已同步的评分可随照片与
              XMP 一同重建。
            </p>
          </>
        )}
        {busy && (
          <p className="backup-busy" role="status">
            <LoaderCircle className="spin" size={14} />
            正在核对，请稍候…
          </p>
        )}
      </section>
    </div>
  );
}
