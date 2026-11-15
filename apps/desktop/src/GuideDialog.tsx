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
            e.preventDefault();
            close.current?.focus();
          }
        }}
      >
        <div className="dialog-heading">
          <BookOpen size={22} />
          <div>
            <span className="eyebrow">A LITTLE HELP, AT YOUR PACE</span>
            <h2>从浏览，到安心整理。</h2>
          </div>
          <button ref={close} className="icon-button" aria-label="关闭使用说明" onClick={onClose}>
            <X size={18} />
          </button>
        </div>
        <div className="guide-body">
          <p className="dialog-intro">
            Moment Grove{info ? ` · ${info.version} · ${info.buildId || 'development'}` : ''} ·
            技术试用版
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
            JPEG，移除源 EXIF/GPS。安全整理会移动测试副本及已有
            XMP；先核对清单再确认，完成后可核对撤回。
            水印样式自动记住，看到“样式已保存”后退出可继续使用；城市和备用日期每批重新填写，历史清单保留原参数。
          </p>
          <h3>更新与恢复</h3>
          <p>
            本版采用手动替换应用。更新前创建“资料库备份”并退出应用，保留旧应用以便回退。快照只包含资料库，不包含照片；若有安全整理，应先撤回再恢复快照。卸载应用不会自动清理资料目录。
          </p>
          <h3>当前范围</h3>
          <p>
            Apple Silicon macOS；JPEG/PNG/WebP、单帧 SDR HEIC/HEIF。预览转为
            sRGB，透明图片使用白底； macOS 14+ 可将已验证的 HDR JPEG/HEIC 解码为 SDR
            预览和水印，不保留 HDR
            显示效果；其他高位深和多帧照片仍不支持。混合目录会说明未载入的视频、RAW
            和其他图片数量；这些文件保留原位。失败时可重试或继续查看其他照片。导出每批最多 100 张；
            水印限静态 SDR、24MP/32MiB。Live Photo/RAW 配对及跨卷移动暂不支持。
          </p>
          <h3>本机资料目录</h3>
          {info ? (
            <>
              <p>
                {info.isolated ? '当前使用独立测试资料目录。' : '当前使用应用默认资料目录。'}
                可选中文字复制到 Finder“前往文件夹”。
              </p>
              <code className="guide-path guide-data-path">{info.dataDirectory}</code>
              <h3>当前应用位置</h3>
              <code className="guide-path">{info.executable || '应用位置不可用'}</code>
              <p>
                同一资料目录重复打开会唤回已有窗口，实际版本以窗口标识为准。更新前先用 ⌘Q
                退出旧版；独立测试与日常资料库互不合并。
              </p>
            </>
          ) : (
            <p>
              {native
                ? error || '正在读取资料位置…'
                : '浏览器预览不使用本机资料库，请在桌面 App 中查看。'}
            </p>
          )}
          <p className="sync-footnote">
            catalog.sqlite 保存评分与队列；cache 可重建；backups 保存快照；exports/moves
            保存恢复记录。请通过应用创建一致快照，不单独复制活动数据库。当前没有自动更新或云同步。
          </p>
        </div>
      </section>
    </div>
  );
}
