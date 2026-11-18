// Each case comes from a real change; the issue number is in its title.
import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { barX, center, cli, open, openEpic } from './helpers';

const tip = (page: import('@playwright/test').Page) => page.locator('#gtip');
/** The page's tooltip is shown with this text (the text alone stays in the hidden box after a previous hover) */
const shows = async (page: import('@playwright/test').Page, text: string) => {
  await expect(tip(page)).toBeVisible();
  await expect(tip(page)).toHaveText(text);
};

test.describe('Gantt tooltips for today and the deadline (#30, #44)', () => {
  test('hovering the today line shows the date on the marker, a story row and a task row', async ({ page }) => {
    await openEpic(page, 'office');
    const today = await center(page, '.today-mark');
    const away = async () => { await page.mouse.move(5, 5); await expect(tip(page)).toBeHidden() };

    // The marker in the header
    await away();
    await page.mouse.move(today.x, today.y);
    await shows(page, 'Today Wed Nov 18');

    // A story row (#44: this one did not show)
    await away();
    const story = await center(page, '.story-row[data-story="S1"] rect');
    await page.mouse.move(today.x, story.y);
    await shows(page, 'Today Wed Nov 18');

    // A task row (away from its bar)
    await away();
    const task = await center(page, 'rect.bar[data-id="BOOK"]');
    expect(task.box.x + task.box.width).toBeLessThan(today.x - 10);
    await page.mouse.move(today.x, task.y);
    await shows(page, 'Today Wed Nov 18');
  });

  test('hovering the deadline line shows the deadline date', async ({ page }) => {
    await openEpic(page, 'office');
    const dl = await center(page, '[data-tip^="Deadline"]');
    await page.mouse.move(5, 5);
    await page.mouse.move(dl.x, dl.y);
    await shows(page, 'Deadline Fri Dec 11');
  });
});

test.describe('Phones (#36)', () => {
  test.use({ viewport: { width: 390, height: 844 } });
  const noPageScroll = async (page: import('@playwright/test').Page) => {
    const w = await page.evaluate(() => ({ doc: document.documentElement.scrollWidth, body: document.body.scrollWidth, view: window.innerWidth }));
    expect(w.doc, 'document wider than the screen').toBeLessThanOrEqual(w.view);
    expect(w.body, 'body wider than the screen').toBeLessThanOrEqual(w.view);
  };
  test('the home page has no page-level horizontal scroll at 390px', async ({ page }) => {
    await open(page, '#/');
    await expect(page.locator('#home-list tr[data-k]').first()).toBeVisible();
    await noPageScroll(page);
  });
  test('the epic page has no page-level horizontal scroll at 390px', async ({ page }) => {
    await openEpic(page, 'office');
    await noPageScroll(page);
  });
});

test.describe('Moves', () => {
  test('dragging a bar moves its start; Save writes it; a reload shows it', async ({ page }) => {
    await openEpic(page, 'drag');
    const before = await center(page, 'rect.bar[data-id="BOOK"]');
    await page.mouse.move(before.x, before.y);
    await page.mouse.down();
    await page.mouse.move(before.x + 60, before.y, { steps: 6 });
    await page.mouse.move(before.x + 120, before.y, { steps: 6 });
    await page.mouse.up();

    // The original position stays as a dashed outline; Save is enabled
    await expect(page.locator('g[data-id="BOOK"] rect[stroke-dasharray="3 3"]')).toHaveCount(1);
    const moved = await barX(page, 'BOOK');
    expect(moved).toBeGreaterThan(before.box.x + 50);
    await expect(page.locator('#save')).toBeEnabled();

    await page.locator('#save').click();
    await expect(page.locator('#toast')).toHaveText('Saved');
    await expect(page.locator('#save')).toBeDisabled();
    expect(cli('dump', 'drag')).toMatch(/id = "BOOK"[\s\S]*?not_before = /);

    await page.reload();
    await expect(page.locator('#gsvg')).toBeVisible();
    expect(await barX(page, 'BOOK')).toBeCloseTo(moved, 0);
    await expect(page.locator('g[data-id="BOOK"] rect[stroke-dasharray="3 3"]')).toHaveCount(0);
  });
});

test.describe('Changes made elsewhere (#34)', () => {
  test('a change by the CLI shows "Updated by cli"; a save on a stale base reports the conflict and keeps the moves', async ({ page }) => {
    await openEpic(page, 'conflict');
    cli('task', 'set', 'conflict', 'PACK', 'effort=2');
    await expect(page.locator('#toast')).toHaveText('Updated by cli', { timeout: 10_000 });

    // Stop the page from noticing changes, so its base goes stale
    await page.route('**/api/version', r => r.abort());
    const bar = await center(page, 'rect.bar[data-id="BOOK"]');
    await page.mouse.move(bar.x, bar.y);
    await page.mouse.down();
    await page.mouse.move(bar.x + 120, bar.y, { steps: 10 });
    await page.mouse.up();
    await expect(page.locator('#save')).toBeEnabled();

    cli('task', 'set', 'conflict', 'MOVE', 'effort=2');
    await page.locator('#save').click();
    await expect(page.locator('#toast')).toContainText('Your moves are kept on the new plan');
    // The moves are still there, on top of the CLI's change, ready to be saved again
    await expect(page.locator('g[data-id="BOOK"] rect[stroke-dasharray="3 3"]')).toHaveCount(1);
    await expect(page.locator('#save')).toBeEnabled();
    expect(cli('dump', 'conflict')).not.toMatch(/id = "BOOK"[\s\S]*?not_before = /);
  });
});

test.describe('Home (#38)', () => {
  test('choosing an owner shows the Next card, which opens the task', async ({ page }) => {
    await open(page, '#/');
    await expect(page.locator('#home-next')).toBeHidden();
    await page.locator('#home-owner').selectOption('bob');
    const card = page.locator('#home-next');
    await expect(card).toBeVisible();
    await expect(card).toContainText('Next for bob');
    await expect(card.locator('.next b')).toHaveText('BOOK Book');
    await card.locator('.next').click();
    await expect(page).toHaveURL(/#\/e\//);
    await expect(page.locator('#gsvg')).toBeVisible();
  });
});

test.describe('Status (#48)', () => {
  test('choosing Blocked in the side panel marks the task and its story; the CLI sees the hold; Done clears it', async ({ page }) => {
    await openEpic(page, 'status');
    await page.locator('g[data-id="PACK"] text').first().click();
    const status = page.locator('#side select[data-f="status"]');
    await expect(status).toHaveValue('todo');
    await status.selectOption('blocked');
    await expect(page.locator('g[data-id="PACK"] .hold-mark')).toHaveText('Blocked');
    await expect(page.locator('.story-row[data-story="S1"] .story-status')).toHaveText('Blocked');

    // the note: who or what it waits for
    const note = page.locator('#side input[data-f="hold_note"]');
    await note.fill('the landlord');
    await note.press('Enter');
    await expect.poll(() => cli('dump', 'status')).toMatch(/id = "PACK"[\s\S]*?hold = \{ kind = "blocked", since = "2026-11-18", note = "the landlord" \}/);
    expect(cli('today')).toContain('PACK blocked since 2026-11-18');

    await page.locator('#side select[data-f="status"]').selectOption('done');
    await expect(page.locator('g[data-id="PACK"] .hold-mark')).toHaveCount(0);
    await expect.poll(() => cli('dump', 'status')).toMatch(/id = "PACK"[\s\S]*?done = true\nactual_finish = "2026-11-18"\n(?!hold)/);
  });
});

test.describe('Status in the Gantt and the network (#49)', () => {
  test('a task in progress gets a solid outline, a mark and flowing dots on its arrow; blocked gets a red outline', async ({ page }) => {
    // BOOK and MOVE are in every fixture epic, so the edits name the epic
    cli('task', 'set', 'visual', 'BOOK', 'actual_start=2026-11-16');
    const ops = path.join(__dirname, '../.tmp/visual-ops.json');
    fs.writeFileSync(ops, JSON.stringify([{ op: 'set_task', key: 'MOVE', patch: { hold: { kind: 'blocked', since: '2026-11-18', note: 'the elevator' } } }]));
    cli('apply', 'visual', ops);
    await openEpic(page, 'visual');
    await expect(page.locator('g[data-id="BOOK"] .st-outline.st-doing')).toHaveCount(1);
    await expect(page.locator('g[data-id="BOOK"] .st-mark.st-doing')).toHaveCount(1);
    await expect(page.locator('g[data-id="BOOK"] .st-progress')).toHaveCount(1);
    await expect(page.locator('g[data-id="MOVE"] .st-outline.st-blocked')).toHaveCount(1);
    await expect(page.locator('.legend').last()).toContainText('In progress');

    await page.keyboard.press('2');
    await expect(page.locator('#nsvg .st-flow')).toHaveCount(1);
    await expect(page.locator('#nsvg g[data-id="MOVE"] .st-box.st-blocked')).toHaveCount(1);
    await expect(page.locator('#nsvg g[data-id="BOOK"] .st-mark.st-doing')).toHaveCount(1);
  });
});

test.describe('Top bar says what each button does', () => {
  test('the light/dark button shows a moon in light and a sun in dark', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await openEpic(page, 'office');
    const theme = page.locator('#top .theme');
    await expect(theme.locator('.moon')).toBeVisible();
    await expect(theme.locator('.sun')).toBeHidden();
    await expect(theme).toHaveAttribute('aria-label', 'Switch to dark');
    await theme.click();
    await expect(theme.locator('.sun')).toBeVisible();
    await expect(theme.locator('.moon')).toBeHidden();
    await expect(theme).toHaveAttribute('aria-label', 'Switch to light');
  });

  test('following a dark system, the button shows a sun', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openEpic(page, 'office');
    await expect(page.locator('#top .theme .sun')).toBeVisible();
    await expect(page.locator('#top .theme')).toHaveAttribute('aria-label', 'Switch to light');
  });

  test('the warning chip names its count and opens the list', async ({ page }) => {
    await openEpic(page, 'demo');
    const warn = page.locator('#warn');
    await expect(warn).toHaveText(/^⚠ \d+ warnings?$/);
    await expect(page.locator('#warn-list')).toBeHidden();
    await warn.click();
    await expect(page.locator('#warn-list li').first()).toBeVisible();
    await expect(warn).toHaveAttribute('aria-expanded', 'true');
    await warn.click();
    await expect(page.locator('#warn-list')).toBeHidden();
  });

  test('undo, redo, Discard and Save appear only after a bar is moved', async ({ page }) => {
    await openEpic(page, 'office');
    await expect(page.locator('#moves')).toBeHidden();
    await page.locator('rect.bar[data-id="BOOK"]').click();
    await page.keyboard.press('ArrowRight');
    await expect(page.locator('#moves')).toBeVisible();
    await expect(page.locator('#save')).toHaveText('Save 1 move');
    await page.locator('#reset').click();
    // Discard can be undone, so the group stays with undo
    await expect(page.locator('#moves')).toBeVisible();
    await expect(page.locator('#undo')).toBeEnabled();
    await page.locator('#undo').click();
    await expect(page.locator('#save')).toHaveText('Save 1 move');
  });
});
