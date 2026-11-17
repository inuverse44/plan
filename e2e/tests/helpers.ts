import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { expect, type Page } from '@playwright/test';

/** The fixed "today" of every case (a Wednesday inside the fixture epics) */
export const TODAY = new Date('2026-11-18T10:00:00+09:00');

const root = path.resolve(__dirname, '../..');

/** Runs the CLI on the test server's SQLite (as another client would) */
export function cli(...args: string[]): string {
  return execFileSync(path.join(root, 'target/release/plan'), args, {
    env: { ...process.env, PLAN_DB: path.join(root, 'e2e/.tmp/plan.db') },
    encoding: 'utf8',
  });
}

/** Fixes the browser's clock at TODAY (timers still run, so the 2-second polling works), keeps the page off the network and opens it */
export async function open(page: Page, hash: string) {
  await page.clock.setFixedTime(TODAY);
  // Only the test server: the cases must not depend on the network (web fonts)
  await page.route(url => url.hostname !== '127.0.0.1', r => r.abort());
  await page.goto('/' + hash);
}

/** Opens an epic and waits for its Gantt */
export async function openEpic(page: Page, key: string) {
  await open(page, `#/e/${key}`);
  await expect(page.locator('#gsvg')).toBeVisible();
}

/** Center of an element's box on the page */
export async function center(page: Page, selector: string) {
  const b = await page.locator(selector).first().boundingBox();
  if (!b) throw new Error(`no box for ${selector}`);
  return { x: b.x + b.width / 2, y: b.y + b.height / 2, box: b };
}

/** Left x of a task's bar, read again until it settles (the Gantt is redrawn after moves and reloads) */
export async function barX(page: Page, id: string): Promise<number> {
  let last = NaN;
  await expect.poll(async () => {
    const b = await page.locator(`rect.bar[data-id="${id}"]`).boundingBox({ timeout: 1000 }).catch(() => null);
    const x = b ? b.x : NaN, same = x === last; last = x;
    return same;
  }, { intervals: [100] }).toBe(true);
  return last;
}
