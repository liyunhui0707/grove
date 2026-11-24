import { dialogControls, TaskError, CopyPath } from './TaskFeedback';
import { useEffect, useRef, useState } from 'react';
import { BookOpen, X } from 'lucide-react';
import { bridge, native } from './bridge';

export function GuideDialog({ onClose }: { onClose: () => void }) {
  const close = useRef<HTMLButtonElement>(null);
  const [info, setInfo] = useState<Awaited<ReturnType<typeof bridge.applicationInfo>> | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    const previous = document.activeElement;
    close.current?.focus();
    let stale = false;
    if (native)
      void bridge.applicationInfo().then(
        (value) => {
          if (!stale) setInfo(value);
        },
        (reason) => {
          if (!stale) setError(String(reason));
        },
      );
    return () => {
      stale = true;
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <section
        className="backup-dialog guide-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="使用说明与资料"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === 'Escape') {
            e.preventDefault();
            onClose();
          }
          if (e.key === 'Tab') {
            const controls = dialogControls(e.currentTarget);
            const first = controls[0],
              last = controls.at(-1);
            if (e.shiftKey && document.activeElement === first) {
              e.preventDefault();
              last?.focus();
            } else if (!e.shiftKey && document.activeElement === last) {
              e.preventDefault();
              first?.focus();
            }
          }
        }}
      >
        <div className="dialog-heading">
          <BookOpen size={22} />
          <div>
            <h2>使用说明与资料</h2>
          </div>
          <button ref={close} className="icon-button" aria-label="关闭使用说明" onClick={onClose}>
            <X size={18} />
          </button>
        </div>
        <div className="guide-body">
          <p className="dialog-intro">
            Grove{info ? ` · ${info.version} · ${info.buildId || 'development'}` : ''} · 技术试用版
          </p>
          <h3>先从测试副本开始</h3>
          <p>
            打开一个照片文件夹，或体验示例图库。浏览不导入原件；评分先保存在本机资料库。审片位置显示已保存后，退出重开会继续上次照片与筛选；目录离线或身份变化时需重新选择。
          </p>
          <h3>挑选与比较</h3>
          <p>
            0 是淘汰候选，1–5 为星级，U 恢复未评分。⌘Z 撤销；选择 2–8 张后按 C
            比较，只对活动照片评分。比较时用 [ / ] 或“上一候选 /
            下一候选”只替换活动照片，其他照片保留；候选来自进入比较时的筛选顺序。键盘操作见左下角“快捷键”。
          </p>
          <h3>同步、导出与整理</h3>
          <p>
            “评分同步”明确写入照片旁的 XMP。“导出副本”保留原字节和元数据；“水印”生成独立
            JPEG，移除源 EXIF/GPS。安全整理会移动所选照片及已有
            XMP；先核对清单再确认，完成后可核对撤回。
            水印样式自动记住，看到“样式已保存”后退出可继续使用；城市和备用日期每批重新填写，历史清单保留原参数。
          </p>
          <h3>淘汰照片与废纸篓</h3>
          <p>
            0
            分只是标记，不会自动删除。进入“淘汰候选”，勾选照片或“全选当前结果”，再点击“移到废纸篓”核对并确认。每批最多
            100 张，已有 XMP 一并处理；关联视频或 RAW 暂不处理。可在 Finder 废纸篓找回照片和
            XMP，放回原位后重新扫描。⌘Z
            只撤销评分，不能撤销移到废纸篓；部分失败或中断须查看“操作记录”。
          </p>
          <h3>更新与恢复</h3>
          <p>
            本版采用手动替换应用。更新前创建“资料库备份”并退出应用，保留旧应用以便回退。快照只包含资料库，不包含照片；若有安全整理，应先撤回再恢复快照；废纸篓中的照片须从
            Finder 放回，资料库快照不能代替照片恢复。卸载应用不会自动清理资料目录。
          </p>
          <h3>当前范围</h3>
          <p>
            Apple Silicon macOS；JPEG/PNG/WebP、单帧 SDR HEIC/HEIF。预览转为
            sRGB，透明图片使用白底； macOS 14+ 可将已验证的 HDR JPEG/HEIC 解码为 SDR
            预览和水印，不保留 HDR
            显示效果；其他高位深和多帧照片仍不支持。混合目录会说明未载入的视频、RAW
            和其他图片数量；这些文件保留原位。失败时可重试或继续查看其他照片。导出每批最多 100 张；
            水印成品为静态 SDR，限 24MP/32MiB；macOS 14+ 可转换已支持的 HDR 照片。
            可复制标识相符的同名 Live Photo 照片与 MOV。RAW 配对、成组移动及跨卷移动暂不支持。
          </p>
          <details className="task-details">
            <summary>版本、资料位置与诊断</summary>
            <h3>本机资料目录</h3>
            {info ? (
              <>
                <p>
                  {info.isolated ? '当前使用独立测试资料目录。' : '当前使用应用默认资料目录。'}
                  路径可选中复制到 Finder“前往文件夹”。
                </p>
                <code className="guide-path guide-data-path">{info.dataDirectory}</code>
                <CopyPath value={info.dataDirectory} />
                <h3>当前应用位置</h3>
                <code className="guide-path">{info.executable || '应用位置不可用'}</code>
                {info.executable && <CopyPath value={info.executable} />}
                {info.startup && (
                  <>
                    <h3>本次启动</h3>
                    <p>
                      {info.startup.ready ? '界面已就绪。' : '尚未收到界面就绪信号。'}
                      {info.startup.recorded
                        ? '本机启动记录已保存。'
                        : '本机启动记录不可写；不影响继续使用。'}
                      startup.json
                      仅保留本次和上次启动的阶段代码、版本与耗时，不记录照片、路径或异常原文，不上传。
                    </p>
                  </>
                )}
                <p>
                  同一资料目录重复打开会唤回已有窗口，实际版本以窗口标识为准。更新前先用 ⌘Q
                  退出旧版；独立测试与日常资料库互不合并。
                </p>
              </>
            ) : (
              <div>
                {native ? (
                  error ? (
                    <TaskError error={error} title="无法读取资料位置" />
                  ) : (
                    '正在读取资料位置…'
                  )
                ) : (
                  '浏览器预览不使用本机资料库，请在桌面 App 中查看。'
                )}
              </div>
            )}
            <p className="sync-footnote">
              catalog.sqlite 保存评分与队列；cache 可重建；backups 保存快照；exports/moves
              保存恢复记录。请通过应用创建一致快照，不单独复制活动数据库。当前没有自动更新或云同步。
            </p>
          </details>
        </div>
      </section>
    </div>
  );
}
