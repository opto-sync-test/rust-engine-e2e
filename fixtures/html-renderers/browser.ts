import { createOptoSyncClient } from '../../vendor/opto-sync-clients/clients/ts/dist/esm/browser.js';

const rendererRoot = document.querySelector('[data-renderer]');
if (!(rendererRoot instanceof HTMLElement)) {
  throw new Error('renderer page omitted a [data-renderer] provenance marker');
}

const renderer = rendererRoot.dataset.renderer ?? 'unknown';
const table = 'renderer_canary';
const recordId = 'shared-record';
const databaseName = `opto-sync-html-renderer-canary-${renderer}`;
const client = await createOptoSyncClient({
  databaseName,
  stampUpdatedAt: false,
});

let authoritative = { title: 'server' };

function element<K extends keyof HTMLElementTagNameMap>(
  name: K,
  id?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(name);
  if (id) node.id = id;
  return node;
}

const island = element('section', 'opto-sync-canary');
island.dataset.renderer = renderer;
island.dataset.ready = 'false';
island.dataset.engineReady = 'false';
island.dataset.pending = '0';
island.dataset.lastMutationId = '';
island.dataset.acknowledged = 'false';

const heading = element('h2');
heading.textContent = 'Opto Sync browser island';
const value = element('output', 'opto-sync-value');
value.setAttribute('aria-live', 'polite');
const mutate = element('button', 'opto-sync-mutate');
mutate.type = 'button';
mutate.textContent = 'Queue local mutation';
const acknowledge = element('button', 'opto-sync-ack');
acknowledge.type = 'button';
acknowledge.textContent = 'Acknowledge mutation';

island.append(heading, value, mutate, acknowledge);
(rendererRoot.querySelector('main') ?? rendererRoot).append(island);

// createOptoSyncClient initializes the browser WASM merge engine. Exercise it
// once so this canary cannot pass with IndexedDB alone while the browser merge
// engine is missing or accidentally routed to the Node native addon.
client.reconcileIncoming(
  table,
  recordId,
  { serverProbe: 'server' },
  { clientProbe: 'client' },
);
island.dataset.engineReady = 'true';

async function render(): Promise<void> {
  const projected = await client.localView(table, recordId, authoritative);
  const pending = await client.pendingMutations(table);
  const recordPending = pending.filter((row) => row.recordId === recordId);
  value.textContent = String(projected.title ?? 'missing');
  island.dataset.pending = String(recordPending.length);
  island.dataset.lastMutationId = recordPending.at(-1)?.mutationId ?? '';
}

mutate.addEventListener('click', () => {
  void (async () => {
    await client.queueMutation(table, recordId, { title: 'client' });
    island.dataset.acknowledged = 'false';
    await render();
  })();
});

acknowledge.addEventListener('click', () => {
  void (async () => {
    const pending = (await client.pendingMutations(table)).filter(
      (row) => row.recordId === recordId,
    );
    const last = pending.at(-1);
    if (!last?.mutationId) return;
    authoritative = JSON.parse(last.jsonPayload) as { title: string };
    await client.confirmSyncedUpTo(last.mutationId, last.clientId);
    island.dataset.acknowledged = 'true';
    await render();
  })();
});

await render();
island.dataset.ready = 'true';

Object.defineProperty(window, '__OPTO_SYNC_HTML_RENDERER_CANARY__', {
  configurable: true,
  value: Object.freeze({ renderer, databaseName }),
});
