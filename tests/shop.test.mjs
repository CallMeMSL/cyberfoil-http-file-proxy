import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";
import { Miniflare, Response, convertV4MiniflareOptions } from "miniflare";

const calls = [];
let upstream;
let iconUpstream;
let testIndex = 0;
let shopOrigin;
const iconBytes = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/l9sAAAAASUVORK5CYII=", "base64");
const runtime = new Miniflare(convertV4MiniflareOptions({
  name: "shop-test",
  inspectorPort: process.env.SHOP_PROFILE ? 0 : undefined,
  compatibilityDate: "2026-09-09",
  modules: [
    { type: "ESModule", path: "build/index.js" },
    { type: "CompiledWasm", path: "build/index_bg.wasm" },
  ],
  outboundService: async (request) => {
    calls.push({ url: request.url, method: request.method, headers: request.headers });
    const url = new URL(request.url);
    if (url.origin === "https://tinfoil.media") {
      assert.match(url.pathname, /^\/thi\/[0-9A-F]{16}\/0\/0\/$/);
      assert.equal(url.search, "");
      return iconUpstream(request);
    }
    assert.equal(url.origin, "https://www.premiumize.me");
    assert.equal(url.pathname, "/api/folder/list");
    return upstream(request);
  },
}));

before(async () => { await runtime.ready; });
after(async () => { await runtime.dispose(); });
beforeEach(() => {
  calls.length = 0;
  shopOrigin = `https://shop-${++testIndex}.example`;
  upstream = () => Response.json({ status: "success", content: [] });
  iconUpstream = () => new Response(null, { status: 404 });
});

function request(path = "/api/shop/sections", credentials = "Games/Switch:test-key", options = {}) {
  const headers = credentials === null ? {} : {
    Authorization: `Basic ${Buffer.from(credentials).toString("base64")}`,
  };
  return runtime.dispatchFetch(`${shopOrigin}${path}`, { headers, ...options });
}

function file(name = "Example.nsp", link = "https://cdn.premiumize.me/a%2Fb.nsp?token=abc%2Fdef&expires=123") {
  return { type: "file", name, size: 5_000_000_000, link };
}

test("modern and root routes return direct links with exactly one API call each", async () => {
  upstream = () => Response.json({ status: "success", content: [
    { type: "folder", name: "Ignored.nsp" }, { type: "file", name: "Notes.txt" }, file(),
  ] });
  for (const path of ["/api/shop/sections", "/"]) {
    const response = await request(path);
    assert.equal(response.status, 200);
    assert.equal(response.headers.get("cache-control"), "private, no-store");
    assert.match(response.headers.get("content-type"), /^application\/json/);
    assert.deepEqual(await response.json(), { sections: [{
      id: "premiumize", title: "Games/Switch",
      items: [{ name: "Example.nsp", size: 5_000_000_000, url: file().link }],
    }] });
  }
  assert.equal(calls.length, 2);
  assert.equal(calls[0].method, "GET");
  assert.equal(calls[0].headers.get("authorization"), "Bearer test-key");
  assert.equal(calls[0].headers.get("accept"), "application/json");
  assert.equal(new URL(calls[0].url).searchParams.get("path"), "Games/Switch");
  assert.ok(!calls[0].url.includes("test-key"));
});

test("missing credentials are challenged without contacting Premiumize", async () => {
  const response = await request("/api/shop/sections", null);
  assert.equal(response.status, 401);
  assert.match(response.headers.get("www-authenticate"), /^Basic /);
  assert.deepEqual(await response.json(), { error: "Invalid credentials" });
  assert.equal(calls.length, 0);
});

test("malformed Basic and empty credentials never reach Premiumize", async () => {
  for (const authorization of ["Bearer key", "Basic !!!", "Basic /w==", "Basic " + Buffer.from(":key").toString("base64")]) {
    const response = await request("/", null, { headers: { Authorization: authorization } });
    assert.equal(response.status, 401);
    await response.text();
  }
  assert.equal(calls.length, 0);
});

test("invalid folder paths are rejected locally", async () => {
  const response = await request("/", "Games/../Other:test-key");
  assert.equal(response.status, 400);
  assert.deepEqual(await response.json(), { error: "Invalid folder path" });
  assert.equal(calls.length, 0);
});

test("folder query preserves Unicode and reserved characters without double decoding", async () => {
  const folder = "Games/\u00dcber + %2F #?&";
  const response = await request("/", `${folder}:test-key`);
  assert.equal(response.status, 200);
  assert.equal((await response.json()).sections[0].title, folder);
  assert.equal(new URL(calls[0].url).searchParams.get("path"), folder);
});

test("explicit root omits the path query and returns an empty section", async () => {
  const response = await request("/", "/:test-key");
  assert.deepEqual(await response.json(), { sections: [{ id: "premiumize", title: "Premiumize", items: [] }] });
  assert.equal(new URL(calls[0].url).search, "");
});

test("unsupported methods and non-shop paths perform no upstream requests", async () => {
  const response = await request("/", "Games:key", { method: "POST", body: "ignored" });
  assert.equal(response.status, 405);
  assert.equal(response.headers.get("allow"), "GET");
  await response.text();
  for (const path of ["/files/example.nsp", "/api/shop/icon/invalid", "/api/saves/list", "/missing"]) {
    const missing = await request(path);
    assert.equal(missing.status, 404);
    await missing.text();
  }
  assert.equal(calls.length, 0);
});

test("title metadata preserves filenames, file IDs, duplicate IDs and direct downloads", async () => {
  const entries = [
    file("Game [010055d009f78000][v0].nsp"),
    file("Game [Language Pack][010055D009F78000].nsz", "https://cdn.example/language.nsz"),
    file("Game+[010055D009F78800][v196608][US].nsp"),
    file("\u00dcber [DLC broken [010055D009F79001]v196608].nsp"),
    file("Game [010055D009F78000] (3.04 GB).NSZ"),
    file("No metadata.nsp"),
    file("Ambiguous [010055D009F78000][010055D009F78800].nsp"),
    file("Invalid [010055D009F780000].nsp"),
  ];
  upstream = () => Response.json({ status: "success", content: entries });
  for (const path of ["/", "/api/shop/sections"]) {
    const response = await request(path);
    assert.equal(response.status, 200);
    const items = (await response.json()).sections[0].items;
    assert.equal(items.length, entries.length);
    for (const [index, entry] of entries.entries()) {
      const titleId = ["010055D009F78000", "010055D009F78000", "010055D009F78800", "010055D009F79001", "010055D009F78000"][index];
      assert.deepEqual(items[index], {
        name: entry.name, size: entry.size, url: entry.link,
        ...(titleId ? { title_id: titleId, icon_url: `/api/shop/icon/${titleId}` } : {}),
      });
    }
  }
  assert.equal(calls.length, 2);
  assert.ok(calls.every(call => new URL(call.url).origin === "https://www.premiumize.me"));
});

test("icons strip client credentials and upstream headers and cache public bytes", async () => {
  iconUpstream = () => new Response(iconBytes, { headers: {
    "Content-Type": "image/png", "Set-Cookie": "private=upstream-secret",
    "Vary": "*", "X-Upstream-Secret": "upstream-secret",
  } });
  const path = "/api/shop/icon/01006f8002326000?url=https://other.example&token=secret";
  const response = await request(path, null, { headers: {
    Authorization: `Basic ${Buffer.from("switch:test-key").toString("base64")}`,
    Cookie: "secret=cookie", UAUTH: "secret-auth", HAUTH: "secret-hash", UID: "secret-console",
    Referer: "https://private.example/?key=secret", Range: "bytes=0-4",
  } });
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("content-type"), "image/png");
  assert.equal(response.headers.get("cache-control"), "public, max-age=86400");
  assert.equal(response.headers.get("content-length"), String(iconBytes.length));
  assert.equal(response.headers.get("x-content-type-options"), "nosniff");
  for (const name of ["set-cookie", "vary", "x-upstream-secret", "www-authenticate"]) {
    assert.equal(response.headers.get(name), null);
  }
  assert.deepEqual(Buffer.from(await response.arrayBuffer()), iconBytes);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "https://tinfoil.media/thi/01006F8002326000/0/0/");
  assert.equal(calls[0].method, "GET");
  for (const name of ["authorization", "cookie", "uauth", "hauth", "uid", "referer", "range"]) {
    assert.equal(calls[0].headers.get(name), null, name);
  }
  iconUpstream = () => { throw new Error("Cache hit must not fetch"); };
  for (const credentials of [null, "other:another-key"]) {
    const cached = await request("/api/shop/icon/01006F8002326000", credentials);
    assert.equal(cached.status, 200);
    assert.deepEqual(Buffer.from(await cached.arrayBuffer()), iconBytes);
  }
  assert.equal(calls.length, 1);
});

test("icons for updates use the base title and share its cache", async () => {
  iconUpstream = () => new Response(iconBytes, { headers: { "Content-Type": "image/png" } });
  for (const titleId of ["01006F8002326800", "01006F8002326000"]) {
    const response = await request(`/api/shop/icon/${titleId}`, null);
    assert.equal(response.status, 200);
    assert.deepEqual(Buffer.from(await response.arrayBuffer()), iconBytes);
  }
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "https://tinfoil.media/thi/01006F8002326000/0/0/");
});

test("icons for DLC prefer their own image", async () => {
  iconUpstream = () => new Response(iconBytes, { headers: { "Content-Type": "image/png" } });
  const response = await request("/api/shop/icon/01006F80023273E8", null);
  assert.equal(response.status, 200);
  assert.deepEqual(Buffer.from(await response.arrayBuffer()), iconBytes);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "https://tinfoil.media/thi/01006F80023273E8/0/0/");
});

test("icons for missing DLC fall back once on source 404 or 500", async () => {
  for (const [index, status] of [404, 500].entries()) {
    const titleId = `01006F800232700${index + 1}`;
    iconUpstream = incoming => new URL(incoming.url).pathname.includes(titleId)
      ? new Response("private failure details", { status })
      : new Response(iconBytes, { headers: { "Content-Type": "image/png" } });
    const response = await request(`/api/shop/icon/${titleId}`, null);
    assert.equal(response.status, 200);
    assert.deepEqual(Buffer.from(await response.arrayBuffer()), iconBytes);
    assert.deepEqual(calls.slice(index * 2).map(call => call.url), [
      `https://tinfoil.media/thi/${titleId}/0/0/`,
      "https://tinfoil.media/thi/01006F8002326000/0/0/",
    ]);
  }
});

test("icons reject malformed routes and unsupported methods without fetching", async () => {
  for (const suffix of ["", "not-an-id", "01006F800232600", "01006F80023260000", "01006F800232600G", "01006F8002326000/", "https%3A%2F%2Fother.example"]) {
    const response = await request(`/api/shop/icon/${suffix}`, null);
    assert.equal(response.status, 404);
    await response.text();
  }
  for (const method of ["POST", "HEAD", "OPTIONS"]) {
    const response = await request("/api/shop/icon/01006F8002326000", null, { method });
    assert.equal(response.status, 405);
    assert.equal(response.headers.get("allow"), "GET");
    await response.text();
  }
  assert.equal(calls.length, 0);
});

test("icon redirects and source errors never follow arbitrary hosts or trigger retries", async () => {
  for (const status of [301, 302, 307, 401, 403, 429, 503]) {
    iconUpstream = () => new Response("secret provider error", {
      status, headers: { Location: "https://other.example/secret" },
    });
    const before = calls.length;
    const response = await request("/api/shop/icon/01006F80023273E8");
    assert.equal(response.status, 502);
    assert.equal(response.headers.get("location"), null);
    assert.equal(response.headers.get("cache-control"), "private, no-store");
    assert.deepEqual(await response.json(), { error: "Icon source is unavailable" });
    assert.equal(calls.length - before, 1);
  }
});

test("missing icons are not cached and cannot fail the catalog", async () => {
  iconUpstream = () => new Response("secret not found", { status: 500 });
  const missing = await request("/api/shop/icon/01006F80023273E8", null);
  assert.equal(missing.status, 404);
  assert.deepEqual(await missing.json(), { error: "Icon not found" });
  assert.equal(calls.length, 2);
  const catalog = await request();
  assert.equal(catalog.status, 200);
  await catalog.json();
  iconUpstream = () => new Response(iconBytes, { headers: { "Content-Type": "image/png" } });
  const recovered = await request("/api/shop/icon/01006F80023273E8", null);
  assert.equal(recovered.status, 200);
  assert.deepEqual(Buffer.from(await recovered.arrayBuffer()), iconBytes);
  assert.equal(calls.length, 4);
});

test("icons reject HTML, SVG, mismatched types and truncated JPEG bodies", async () => {
  for (const [body, contentType] of [
    ["<html>secret</html>", "text/html"], ["<html>secret</html>", "image/jpeg"],
    ["<svg></svg>", "image/svg+xml"], [iconBytes, "image/jpeg"],
    [Buffer.from([0xFF, 0xD8, 0xFF]), "image/jpeg"], ["", "image/png"],
  ]) {
    iconUpstream = () => new Response(body, { headers: { "Content-Type": contentType } });
    const response = await request("/api/shop/icon/01006F8002326000", null);
    assert.equal(response.status, 502);
    assert.deepEqual(await response.json(), { error: "Invalid image from icon source" });
  }
});

test("icon response size is bounded for declared and streamed bodies", async () => {
  for (const declared of [true, false]) {
    iconUpstream = () => new Response(new ReadableStream({
      start(controller) {
        controller.enqueue(iconBytes);
        controller.enqueue(new Uint8Array(2 * 1024 * 1024));
        controller.close();
      },
    }), { headers: {
      "Content-Type": "image/png",
      ...(declared ? { "Content-Length": String(2 * 1024 * 1024 + iconBytes.length) } : {}),
    } });
    const response = await request("/api/shop/icon/01006F8002326000", null);
    assert.equal(response.status, 502);
    assert.deepEqual(await response.json(), { error: "Icon exceeds the 2 MiB limit" });
  }
});

test("icons have one six-second deadline including a stalled fallback body", { timeout: 10_000 }, async () => {
  iconUpstream = incoming => new URL(incoming.url).pathname.includes("01006F80023273E8")
    ? new Response(null, { status: 404 })
    : new Response(new ReadableStream({ start(controller) { controller.enqueue(iconBytes); } }), {
      headers: { "Content-Type": "image/png" },
    });
  const started = performance.now();
  const response = await request("/api/shop/icon/01006F80023273E8", null);
  assert.equal(response.status, 504);
  assert.deepEqual(await response.json(), { error: "Icon request timed out" });
  assert.equal(calls.length, 2);
  assert.ok(performance.now() - started < 8500);
});

test("HTTP 200 API error envelopes map to safe errors without raw messages", async () => {
  for (const [code, status] of [
    ["authentication_failed", 401], ["permission_denied", 403], ["not_found", 404],
    ["invalid_request", 400], ["rate_limit_reached", 429], ["account_limit_reached", 429],
    ["service_limit_reached", 429], ["service_down", 503], ["transient_error", 502], ["unknown_error", 502],
  ]) {
    upstream = () => Response.json({ status: "error", code, message: "test-key https://secret.example" });
    const response = await request();
    assert.equal(response.status, status, code);
    const body = await response.text();
    assert.ok(!body.includes("test-key"));
    assert.ok(!body.includes("secret.example"));
  }
  assert.equal(calls.length, 10);
});

test("upstream HTTP failures are not mistaken for catalogs", async () => {
  for (const [status, expected] of [[401, 401], [403, 403], [404, 404], [429, 429], [500, 502], [503, 503], [504, 504]]) {
    upstream = () => new Response("secret upstream body", { status });
    const response = await request();
    assert.equal(response.status, expected);
    assert.ok(!(await response.text()).includes("secret upstream body"));
  }
});

test("retry-after is preserved only when valid", async () => {
  for (const value of ["120", "Wed, 09 Sep 2026 12:00:00 GMT", "unsafe-value"]) {
    upstream = () => Response.json({ status: "error", code: "rate_limit_reached" }, { headers: { "Retry-After": value } });
    const response = await request();
    assert.equal(response.status, 429);
    assert.equal(response.headers.get("retry-after"), value === "unsafe-value" ? null : value);
    await response.text();
  }
});

test("upstream redirects are rejected without forwarding the Bearer key", async () => {
  upstream = () => new Response(null, { status: 302, headers: { Location: "https://other.example/login" } });
  const response = await request();
  assert.equal(response.status, 502);
  assert.equal(response.headers.get("location"), null);
  await response.text();
  assert.equal(calls.length, 1);
});

test("malformed JSON and missing success content are rejected", async () => {
  for (const body of ["<html>login secret</html>", "{", '{"status":"success"}', '{"content":[]}']) {
    upstream = () => new Response(body);
    const response = await request();
    assert.equal(response.status, 502);
    assert.deepEqual(await response.json(), { error: "Invalid response from Premiumize" });
  }
});

test("missing file metadata and unsafe URLs cannot become shop items", async () => {
  for (const entry of [
    { ...file(), link: undefined }, { ...file(), size: undefined }, { ...file(), size: -1 },
    file("Game.nsp", "http://cdn.example/game.nsp"), file("Game.nsp", "/relative.nsp"),
    file("Game.nsp", "https://user:secret@cdn.example/game.nsp"),
  ]) {
    upstream = () => Response.json({ status: "success", content: [entry] });
    const response = await request();
    assert.equal(response.status, 502);
    await response.text();
  }
});

test("response size is bounded with and without content-length", async () => {
  const oversized = " ".repeat(4 * 1024 * 1024 + 1);
  for (const withLength of [true, false]) {
    upstream = () => new Response(new ReadableStream({
      start(controller) {
        const chunk = new TextEncoder().encode(oversized);
        controller.enqueue(chunk);
        controller.close();
      },
    }), { headers: withLength ? { "Content-Length": String(oversized.length) } : {} });
    const response = await request();
    assert.equal(response.status, 502);
    assert.match((await response.json()).error, /4 MiB/);
  }
});

test("concurrent requests do not share credentials or folder contents", async () => {
  upstream = (incoming) => Response.json({ status: "success", content: [
    file(`${new URL(incoming.url).searchParams.get("path")}.nsp`),
  ] });
  const responses = await Promise.all([request("/", "First:first-key"), request("/", "Second:second-key")]);
  const catalogs = await Promise.all(responses.map((response) => response.json()));
  assert.deepEqual(catalogs.map((catalog) => catalog.sections[0].items[0].name), ["First.nsp", "Second.nsp"]);
  assert.deepEqual(calls.map((call) => [new URL(call.url).searchParams.get("path"), call.headers.get("authorization")]).sort(),
    [["First", "Bearer first-key"], ["Second", "Bearer second-key"]]);
});

test("1000-file catalog stays compact and uses one upstream call", async (context) => {
  const content = Array.from({ length: 1000 }, (_, index) => file(`Example ${index} ${"name".repeat(20)} [010055D009F78000].nsp`,
    `https://cdn.premiumize.me/${index}/example.nsp?token=${"abcdef".repeat(32)}`));
  upstream = () => Response.json({ status: "success", content });
  const started = performance.now();
  const response = await request();
  const body = await response.text();
  assert.equal(response.status, 200);
  assert.equal(JSON.parse(body).sections[0].items.length, 1000);
  assert.ok(JSON.parse(body).sections[0].items.every(item => item.title_id === "010055D009F78000"));
  assert.ok(Buffer.byteLength(body) < 512 * 1024);
  assert.equal(calls.length, 1);
  context.diagnostic(`1000 items: ${Buffer.byteLength(body)} bytes; ${(performance.now() - started).toFixed(1)} ms local end-to-end time (not Cloudflare CPU time).`);
});

test("the 20-second deadline includes a stalled response body", { timeout: 25_000 }, async () => {
  upstream = () => new Response(new ReadableStream({
    start(controller) { controller.enqueue(new TextEncoder().encode('{"status":"success","content":[')); },
  }));
  const response = await request();
  assert.equal(response.status, 504);
  assert.deepEqual(await response.json(), { error: "Premiumize request timed out" });
  assert.equal(calls.length, 1);
});

test("local CPU profile for a 1000-file catalog", { skip: !process.env.SHOP_PROFILE, timeout: 15_000 }, async (context) => {
  const inspectorAddress = await runtime.getInspectorURL();
  inspectorAddress.protocol = "http:";
  const targets = await (await fetch(new URL("/json/list", inspectorAddress))).json();
  const target = targets.find((entry) => entry.id === "core:user:shop-test");
  assert.ok(target, "The shop Worker must expose an inspector target");
  const inspector = new WebSocket(target.webSocketDebuggerUrl);
  context.after(() => inspector.close());
  await new Promise((resolve, reject) => {
    inspector.addEventListener("open", resolve, { once: true });
    inspector.addEventListener("error", reject, { once: true });
  });
  let nextId = 0;
  const pending = new Map();
  inspector.addEventListener("message", (event) => {
    const response = JSON.parse(event.data);
    const callback = pending.get(response.id);
    if (callback) {
      pending.delete(response.id);
      if (response.error) callback.reject(new Error(response.error.message));
      else callback.resolve(response.result);
    }
  });
  function command(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = ++nextId;
      pending.set(id, { resolve, reject });
      inspector.send(JSON.stringify({ id, method, params }));
    });
  }
  const content = Array.from({ length: 1000 }, (_, index) => file(`Example ${index} ${"name".repeat(20)} [010055D009F78000].nsp`,
    `https://cdn.premiumize.me/${index}/example.nsp?token=${"abcdef".repeat(32)}`));
  upstream = () => Response.json({ status: "success", content });
  for (let warmup = 0; warmup < 5; warmup++) await (await request()).text();
  await command("Profiler.enable");
  await command("Profiler.setSamplingInterval", { interval: 100 });
  await command("Profiler.start");
  const iterations = 30;
  for (let iteration = 0; iteration < iterations; iteration++) {
    const response = await request();
    assert.equal(response.status, 200);
    await response.text();
  }
  const { profile } = await command("Profiler.stop");
  const nodes = new Map(profile.nodes.map((entry) => [entry.id, entry.callFrame.functionName]));
  const activeMicroseconds = profile.samples.reduce((sum, id, index) =>
    ["(idle)", "(program)", "(root)"].includes(nodes.get(id)) ? sum : sum + profile.timeDeltas[index], 0);
  const heap = await command("Runtime.getHeapUsage");
  assert.equal(calls.length, 35);
  context.diagnostic(`Local sampled active time: ${(activeMicroseconds / iterations / 1000).toFixed(2)} ms/request over ${iterations} warmed requests. This is a profiler estimate, not deployed Cloudflare CPU accounting.`);
  context.diagnostic(`Inspector memory: ${JSON.stringify(heap)}. Values are bytes; heap and backing storage are reported separately.`);
});