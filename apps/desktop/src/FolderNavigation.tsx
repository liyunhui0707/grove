import { useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, Folder } from 'lucide-react';
import type { Asset } from './domain';

interface Branch {
  path: string;
  name: string;
  children: Map<string, Branch>;
}

function buildTree(name: string, photos: Asset[]) {
  const root: Branch = { path: '', name, children: new Map() };
  for (const photo of photos) {
    let parent = root;
    for (const part of photo.relative.split('/').slice(0, -1)) {
      let next = parent.children.get(part);
      if (!next) {
        next = {
          path: parent.path ? parent.path + '/' + part : part,
          name: part,
          children: new Map(),
        };
        parent.children.set(part, next);
      }
      parent = next;
    }
  }
  return root;
}

export function FolderNavigation({
  name,
  photos,
  branch,
  onChoose,
  disabled,
}: {
  name: string;
  photos: Asset[];
  branch: string;
  onChoose: (path: string) => void;
  disabled: boolean;
}) {
  const root = useMemo(() => buildTree(name, photos), [name, photos]);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  function render(node: Branch, depth: number) {
    const open = expanded[node.path] ?? (!node.path || branch.startsWith(node.path + '/'));
    return (
      <li key={node.path}>
        <div className="folder-row" style={{ paddingInlineStart: Math.min(depth, 5) * 10 }}>
          {node.children.size > 0 ? (
            <button
              className="folder-toggle"
              aria-label={(open ? '折叠 ' : '展开 ') + node.name}
              aria-expanded={open}
              onClick={() => setExpanded((current) => ({ ...current, [node.path]: !open }))}
            >
              {open ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
            </button>
          ) : (
            <span className="folder-spacer" />
          )}
          <button
            className="folder-link"
            aria-current={branch === node.path ? 'location' : undefined}
            disabled={disabled}
            title={node.path || name}
            onClick={() => onChoose(node.path)}
          >
            <Folder size={15} />
            <span>{node.name}</span>
          </button>
        </div>
        {open && node.children.size > 0 && (
          <ul>
            {Array.from(node.children.values())
              .sort((a, b) => a.name.localeCompare(b.name, 'zh-CN', { numeric: true }))
              .map((child) => render(child, depth + 1))}
          </ul>
        )}
      </li>
    );
  }
  return (
    <nav className="folder-tree" aria-label="文件夹导航">
      <ul>{render(root, 0)}</ul>
    </nav>
  );
}
