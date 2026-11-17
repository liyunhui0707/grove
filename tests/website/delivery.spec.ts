import { test, expect } from '@playwright/test';

for (const width of [360, 390, 768, 900, 901, 1440]) {
  test(`product page is usable at ${width}px and keeps exact published downloads`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    const failures: string[] = [];
    page.on('pageerror', (e) => failures.push(e.message));
    await page.route('https://**', (route) => route.abort()); // No remote upload/download during local QA.
    await page.goto('/');
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    const layout = await page.evaluate(() => ({
      width: innerWidth,
      scroll: document.documentElement.scrollWidth,
      overflow: [...document.querySelectorAll('body *')]
        .map((el) => ({
          tag: el.tagName,
          class: el.className,
          right: el.getBoundingClientRect().right,
        }))
        .filter((el) => el.right > innerWidth + 1)
        .slice(0, 12),
    }));
    expect(layout.scroll, JSON.stringify(layout)).toBeLessThanOrEqual(layout.width);
    const demo = page.getByRole('group', { name: '网页评分演示' });
    for (const rating of [0, 1, 2, 3, 4, 5]) {
      const button = demo.locator(`[data-rating="${rating}"]`);
      await button.click();
      await expect(page.locator('#rating-state')).toHaveText(
        rating === 0 ? '淘汰候选' : `${rating} 星`,
      );
      await expect(button).toHaveAttribute('aria-pressed', 'true');
      await expect(demo.locator('[aria-pressed="true"]')).toHaveCount(1);
    }
    await demo.getByRole('button', { name: '恢复未评分' }).focus();
    await page.keyboard.press('Enter');
    await expect(page.locator('#rating-state')).toHaveText('未评分');
    await expect(demo.locator('[aria-pressed="true"]')).toHaveCount(0);
    await page.getByRole('link', { name: '下载 macOS 测试版' }).click();
    const dmg = page.locator('[data-download="dmg"]');
    await expect(dmg).toBeInViewport();
    for (const ext of ['dmg', 'zip'])
      await expect(page.locator(`[data-download="${ext}"]`)).toHaveAttribute(
        'href',
        `https://github.com/liyunhui0707/moment-grove/releases/download/v0.8.0/Moment-Grove-0.8.0-macOS-arm64.${ext}`,
      );
    await expect(page.locator('#download')).toContainText('未完成 Developer ID');
    const summary = page.locator('details summary').first();
    await summary.focus();
    await page.keyboard.press('Enter');
    await expect(page.locator('details').first()).toHaveAttribute('open', '');
    const shot = page.locator('.product-shot img');
    await shot.scrollIntoViewIfNeeded();
    await expect(shot).toHaveJSProperty('complete', true);
    expect(await shot.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBeGreaterThan(0);
    await page.evaluate(() => scrollTo(0, 0));
    await page.screenshot({ path: testInfo.outputPath(`site-${width}.png`), fullPage: true });
    expect(failures).toEqual([]);
  });
}
