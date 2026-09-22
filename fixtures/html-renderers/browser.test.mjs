import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const origin = 'http://127.0.0.1:38081';
const bundle = await readFile(
  resolve('fixtures/html-renderers/client.js'),
  'utf8',
);
const requireFromClient = createRequire(
  pathToFileURL(
    resolve('vendor/opto-sync-clients/clients/ts/package.json'),
  ),
);
const { chromium } = requireFromClient('playwright');

const renderers = [
  { name: 'MASH', path: '/htmx/insights', marker: 'maud' },
  { name: 'Leptos', path: '/leptos/insights', marker: 'leptos' },
  { name: 'Dioxus', path: '/dioxus/insights', marker: 'dioxus' },
];

for (const renderer of renderers) {
  const response = await fetch(`${origin}${renderer.path}`);
  assert.equal(response.status, 200, `${renderer.name} SSR status`);
  const serverHtml = await response.text();
  assert.match(
    serverHtml,
    new RegExp(`data-renderer=["']${renderer.marker}["']`),
    `${renderer.name} must carry exact server-renderer provenance`,
  );
  assert.equal(
    serverHtml.includes('opto-sync-canary'),
    false,
    `${renderer.name} browser island must not be pre-rendered by the server`,
  );
}

const browser = await chromium.launch({ headless: true });
try {
  for (const renderer of renderers) {
    const context = await browser.newContext();
    const page = await context.newPage();
    const browserErrors = [];
    page.on('pageerror', (error) => browserErrors.push(String(error)));

    await page.goto(`${origin}${renderer.path}`, { waitUntil: 'domcontentloaded' });
    await page.addScriptTag({ content: bundle, type: 'module' });
    await page.locator('#opto-sync-canary[data-ready="true"]').waitFor();

    assert.equal(
      await page.locator('#opto-sync-canary').getAttribute('data-renderer'),
      renderer.marker,
      `${renderer.name} browser code must bind to the SSR renderer it received`,
    );
    assert.equal(
      await page.locator('#opto-sync-value').textContent(),
      'server',
      `${renderer.name} starts from authoritative server state`,
    );
    assert.equal(
      await page.locator('#opto-sync-canary').getAttribute('data-pending'),
      '0',
      `${renderer.name} starts with an empty durable queue`,
    );

    await page.locator('#opto-sync-mutate').click();
    await page.locator('#opto-sync-value').filter({ hasText: 'client' }).waitFor();
    await page.waitForFunction(
      () => document.querySelector('#opto-sync-canary')?.getAttribute('data-pending') === '1',
    );
    const mutationId = await page
      .locator('#opto-sync-canary')
      .getAttribute('data-last-mutation-id');
    assert.ok(mutationId, `${renderer.name} must expose a durable mutation identity`);

    await page.reload({ waitUntil: 'domcontentloaded' });
    assert.equal(
      await page.locator('#opto-sync-canary').count(),
      0,
      `${renderer.name} Opto Sync UI is client-rendered, not server-smuggled`,
    );
    await page.addScriptTag({ content: bundle, type: 'module' });
    await page.locator('#opto-sync-canary[data-ready="true"]').waitFor();
    assert.equal(
      await page.locator('#opto-sync-value').textContent(),
      'client',
      `${renderer.name} must replay the IndexedDB mutation after reload`,
    );
    assert.equal(
      await page.locator('#opto-sync-canary').getAttribute('data-pending'),
      '1',
      `${renderer.name} reload must retain the durable pending queue`,
    );
    assert.equal(
      await page.locator('#opto-sync-canary').getAttribute('data-last-mutation-id'),
      mutationId,
      `${renderer.name} reload must preserve stable mutation identity`,
    );

    await page.locator('#opto-sync-ack').click();
    await page.waitForFunction(
      () => document.querySelector('#opto-sync-canary')?.getAttribute('data-pending') === '0',
    );
    assert.equal(
      await page.locator('#opto-sync-canary').getAttribute('data-acknowledged'),
      'true',
      `${renderer.name} acknowledgment must settle the durable queue`,
    );
    assert.equal(
      await page.locator('#opto-sync-value').textContent(),
      'client',
      `${renderer.name} acknowledged value must remain externally visible`,
    );
    assert.deepEqual(browserErrors, [], `${renderer.name} browser errors`);

    await context.close();
  }
} finally {
  await browser.close();
}

console.log(
  JSON.stringify({
    schema: 'opto-sync-html-renderer-canary.v1',
    serverRenderers: renderers.map(({ name, path, marker }) => ({ name, path, marker })),
    browserEngine: '@opto-sync/client browser/WASM + real IndexedDB',
    verified: [
      'server-rendered HTML provenance',
      'client-side DOM island',
      'browser WASM client initialization',
      'durable IndexedDB mutation identity',
      'pending mutation replay after reload',
      'acknowledgement settlement',
    ],
    notClaimed: [
      'Leptos framework-native WASM hydration',
      'Dioxus framework-native WASM hydration',
      'production backend delivery',
      'physical-device evidence',
    ],
  }),
);
