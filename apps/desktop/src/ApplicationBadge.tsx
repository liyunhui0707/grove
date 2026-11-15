import { useEffect, useState } from 'react';
import { bridge, native } from './bridge';

export function ApplicationBadge({ onOpen }: { onOpen: (button: HTMLButtonElement) => void }) {
  const [info, setInfo] = useState<Awaited<ReturnType<typeof bridge.applicationInfo>> | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let stale = false;
    if (native)
      void bridge.applicationInfo().then(
        (value) => {
          if (!stale) setInfo(value);
        },
        () => {
          if (!stale) setFailed(true);
        },
      );
    return () => {
      stale = true;
    };
  }, []);
  return (
    <button
      className="application-badge"
      onClick={(e) => onOpen(e.currentTarget)}
      title={info ? `${info.buildId} · 点击查看应用与资料位置` : '查看应用与资料位置'}
    >
      {info
        ? `${info.version} · ${info.isolated ? '独立测试' : '日常资料库'}`
        : native
          ? failed
            ? '应用信息读取失败'
            : '读取应用信息…'
          : '浏览器演示'}
    </button>
  );
}
