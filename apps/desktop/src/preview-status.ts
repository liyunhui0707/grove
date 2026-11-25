import { createContext } from 'react';

// Native failures carry a human-readable reason. Unknown failures stay retryable.
export function previewProblem(message: string) {
  if (/离线|已移动|身份已变化|无法读取文件身份|选择器打开|不可读/.test(message))
    return { label: '原文件不可用', action: 'rescan' } as const;
  if (/限单张|超过.*限制|超过.*像素|预览限|内存上限/.test(message))
    return { label: '超过预览上限', action: 'skip' } as const;
  if (/损坏|不完整|无法识别照片编码|照片元数据无法读取/.test(message))
    return { label: '无法读取图像数据', action: 'skip' } as const;
  if (/多帧|动态 PNG|HDR|高位深|仅转换|尚未验证|色彩空间|未返回受支持/.test(message))
    return { label: '暂不支持此编码', action: 'skip' } as const;
  return { label: '预览暂不可用', action: 'retry' } as const;
}

export const PreviewReport = createContext<(id: string, size: number, reason: string) => void>(
  () => {},
);
