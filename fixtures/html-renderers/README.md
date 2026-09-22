# Opto Sync HTML renderer canary

This canary qualifies the HTML boundary between the exact Opto Sync browser
client and three real Rust server renderers:

- MASH-style Maud + Axum + SeaORM + htmx (`/htmx/insights`),
- Leptos SSR (`/leptos/insights`),
- Dioxus SSR (`/dioxus/insights`).

The workflow pins both authorities by immutable commit SHA. The server adapter
starts the existing unified Axum renderer graph with a disconnected SeaORM
handle and no credentials; signed-out page rendering therefore exercises real
HTTP and real renderer code without mutating a database or running migrations.

For each renderer a real Chromium page then loads the server-rendered HTML and
injects a renderer-neutral browser island bundled from `@opto-sync/client`.
The island:

1. initializes the browser/WASM merge engine,
2. opens the real Dexie/IndexedDB Opto Sync queue,
3. renders the authoritative server projection,
4. queues an optimistic mutation with a stable mutation id,
5. reloads the server-rendered page,
6. proves the pending mutation survives and is replayed over server state, and
7. acknowledges the mutation and proves the durable queue settles without the
   visible value reverting.

This deliberately tests Opto Sync as a framework-neutral replication boundary:
server renderers may produce different HTML implementations, but they must not
cause the browser sync client to touch browser-only APIs during SSR or lose its
durable queue when the page is replaced.

## Evidence boundary

Passing this canary proves server-rendered HTML coexistence plus an actual
client-side Opto Sync/WASM/IndexedDB island on each renderer's HTML. It does
**not** claim Leptos-native WASM hydration, Dioxus-native WASM hydration,
production backend delivery, or physical-device evidence. Those require
separate exact-runtime lanes rather than being inferred from DOM compatibility.

The generated fleet workflow is intentionally left untouched; this is a focused
product canary beside it because `.github/workflows/integration.yml` is owned by
`zed-pkg-test/test-org-fleet`.
