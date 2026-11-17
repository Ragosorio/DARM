/*
 * DARM reader service worker: makes the application shell work offline after
 * the first visit. Dependency-free and deliberately small.
 *
 * Precache (install)
 *   The production build writes `precache.json` next to index.html: a JSON
 *   array of relative URLs covering the whole shell (index.html, scripts,
 *   styles, the WebAssembly core, fonts, icons, the manifest). On install the
 *   worker fetches that list and stores every entry. This matters on a first
 *   visit: the page loaded its files before any worker controlled it, so a
 *   cache-on-fetch worker would have nothing to serve on an offline reload.
 *   A missing optional file does not abort the install.
 *
 *   The cache is named after a hash of the list, so a new build gets a new
 *   cache; the previous ones are deleted when the new worker activates. The
 *   list is also re-checked after each successful online navigation, so a new
 *   build is picked up even though this file's own bytes did not change.
 *
 * Runtime
 *   - assets/ (content-hashed by Vite): cache-first.
 *   - navigations to the app (`./`, `./index.html`, with or without a query
 *     string): network-first, falling back to the cached index.html.
 *   - any other same-origin GET in scope: network; if the network fails and
 *     the file was precached (manifest, icons) the stored copy is served.
 *     Nothing that was not in the precache list is ever stored here.
 *
 * Never touched (the request goes to the network as if there were no worker,
 * and nothing is stored)
 *   - anything that is not a same-origin GET inside this worker's scope;
 *   - /__darm/* (the `darm open` / `darm dev` preview server);
 *   - books: any *.darm URL and any Range request. The reader keeps the
 *     library in IndexedDB/OPFS, not in this cache.
 *
 * The app registers this file only in production builds, in a secure context
 * (https, or http://localhost / 127.0.0.1 under `vite preview`), so `vite dev`
 * is unaffected.
 */
const PREFIX = "darm-shell-";
/** Holds one entry: the name of the cache that is complete and in use. */
const META = "darm-sw-meta";
const SCOPE = new URL(self.registration.scope);
const SHELL = new URL("./", SCOPE).href;
const INDEX = new URL("index.html", SCOPE).href;
const ASSETS = new URL("assets/", SCOPE).pathname;
const LIST = new URL("precache.json", SCOPE).href;
const CURRENT = new URL("__sw-current", SCOPE).href;
const PREVIEW = "/__darm/";
/** Used when the build has no precache.json: the shell plus whatever is requested later. */
const FALLBACK = PREFIX + "runtime";

/** Name of the cache in use, remembered for the lifetime of this worker instance. */
let current = null;
/** The precache run in flight, if any, so two triggers do not race. */
let refreshing = null;

self.addEventListener("install", (event) => {
  // A failed precache (offline, storage full) must not stop the worker from
  // installing: it will be tried again after the next online navigation.
  event.waitUntil(
    refresh()
      .catch(() => undefined)
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      await prune(await currentCache());
      await self.clients.claim();
    })(),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (!url.pathname.startsWith(SCOPE.pathname)) return;
  if (url.pathname.includes(PREVIEW)) return;
  if (request.headers.has("range")) return;
  if (/\.darm$/i.test(url.pathname)) return;
  if (url.href === LIST || url.pathname === self.location.pathname) return;

  if (url.pathname.startsWith(ASSETS)) {
    event.respondWith(cacheFirst(request));
  } else if (isShell(url)) {
    // `./`, `./index.html` and `./?anything` are all the same document.
    event.respondWith(
      networkFirst(request, SHELL, true).then(({ response, online }) => {
        // Online: see whether a new build has been published since.
        if (online) event.waitUntil(refresh().then(prune, () => undefined));
        return response;
      }),
    );
  } else {
    event.respondWith(networkFirst(request, url.origin + url.pathname, false).then(({ response }) => response));
  }
});

function isShell(url) {
  return url.pathname === SCOPE.pathname || url.pathname === SCOPE.pathname + "index.html";
}

/** Only complete, successful, same-origin responses are ever stored. */
function storable(response, url) {
  if (response.status !== 200 || response.type !== "basic") return false;
  // A static host with a single-page fallback answers a missing asset with
  // index.html and status 200. That must never be stored as a script or font.
  const type = response.headers.get("content-type") || "";
  return !(new URL(url).pathname.startsWith(ASSETS) && type.includes("text/html"));
}

async function currentCache() {
  if (current) return current;
  try {
    const meta = await caches.open(META);
    const entry = await meta.match(CURRENT);
    current = entry ? await entry.text() : FALLBACK;
  } catch {
    current = FALLBACK;
  }
  return current;
}

async function put(key, response) {
  try {
    const cache = await caches.open(await currentCache());
    await cache.put(key, response);
  } catch {
    // Storage full or unavailable (private browsing): keep working from the network.
  }
}

/*
 * Every lookup ignores `Vary`. Dev and static servers commonly answer with
 * `Vary: Origin`; the precache request carries no Origin header while a
 * `<script type="module" crossorigin>` request in Chromium does, so a
 * Vary-respecting match would miss the very files the page needs offline.
 * The files are content-hashed or app-owned, so one stored copy is right for
 * every request.
 */
async function cacheFirst(request) {
  const cached = await caches.match(request, { cacheName: await currentCache(), ignoreVary: true });
  if (cached) return cached;
  const response = await fetch(request);
  if (storable(response, request.url)) await put(request, response.clone());
  return response;
}

/**
 * `key` is the URL the response is stored under (without a query string).
 * With `always` false the cached copy is refreshed only if the file was
 * precached, so arbitrary URLs are never added to the cache.
 */
async function networkFirst(request, key, always) {
  const name = await currentCache();
  try {
    const response = await fetch(request);
    if (storable(response, key) && (always || (await caches.match(key, { cacheName: name, ignoreVary: true })))) {
      await put(key, response.clone());
    }
    return { response, online: true };
  } catch (error) {
    const cached = await caches.match(key, { cacheName: name, ignoreVary: true });
    if (!cached) throw error;
    return { response: cached, online: false };
  }
}

/** Deletes every shell cache except `keep`. */
async function prune(keep) {
  for (const key of await caches.keys()) {
    if (key.startsWith(PREFIX) && key !== keep) await caches.delete(key);
  }
  return keep;
}

async function shortHash(text) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return Array.from(new Uint8Array(digest).slice(0, 8), (b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * Makes sure the cache for the published build exists and is in use.
 * Resolves to its name. Rejects (changing nothing) when the network or the
 * shell itself is unavailable.
 */
function refresh() {
  if (!refreshing) {
    refreshing = precache().finally(() => {
      refreshing = null;
    });
  }
  return refreshing;
}

async function precache() {
  // A network error here rejects: offline, keep whatever is in use.
  const listed = await fetch(LIST, { cache: "no-store" });
  let entries = null;
  let name = FALLBACK;
  if (listed.ok) {
    const text = await listed.text();
    try {
      const parsed = JSON.parse(text);
      if (Array.isArray(parsed)) {
        entries = parsed.filter((entry) => typeof entry === "string");
        name = PREFIX + (await shortHash(text));
      }
    } catch {
      // Not JSON (for example a single-page fallback answered with index.html).
    }
  }
  if (name === (await currentCache()) && (await caches.has(name)) && name !== FALLBACK) return name;

  const urls = new Set([SHELL]);
  for (const entry of entries ?? []) {
    let url;
    try {
      url = new URL(entry, SCOPE);
    } catch {
      continue;
    }
    url.hash = "";
    if (url.origin !== self.location.origin || !url.pathname.startsWith(SCOPE.pathname)) continue;
    if (url.pathname.includes(PREVIEW) || /\.darm$/i.test(url.pathname)) continue;
    if (url.href === LIST || url.pathname === self.location.pathname) continue;
    urls.add(url.href === INDEX ? SHELL : url.href);
  }

  const cache = await caches.open(name);
  const add = async (href) => {
    // Hashed assets may come from the HTTP cache (the page has just downloaded
    // them); everything else is revalidated.
    const hashed = new URL(href).pathname.startsWith(ASSETS);
    const response = await fetch(href, { cache: hashed ? "default" : "no-cache" });
    if (!storable(response, href)) throw new Error("not storable: " + href);
    await cache.put(href, response);
  };
  // The shell is mandatory; everything else is best effort.
  await add(SHELL);
  urls.delete(SHELL);
  const queue = [...urls];
  while (queue.length > 0) {
    await Promise.allSettled(queue.splice(0, 8).map(add));
  }

  const meta = await caches.open(META);
  await meta.put(CURRENT, new Response(name));
  current = name;
  return name;
}
