import type { Page } from '@playwright/test';
import type { SavedWorkspace } from '../../apps/desktop/src/workspace';

export function savedView(page: Page) {
  return page.evaluate(() => {
    const workspace: SavedWorkspace | null = JSON.parse(
      localStorage.getItem('workspace') || 'null',
    );
    return workspace?.view;
  });
}
