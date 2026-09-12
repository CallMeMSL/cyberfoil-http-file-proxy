# Premiumize Cyberfoil Shop

A Rust Cloudflare Worker using [workers-rs](https://github.com/cloudflare/workers-rs) to expose one Premiumize cloud folder as a Cyberfoil shop.

The Worker serves **catalog JSON only**. Cyberfoil downloads files directly from the HTTPS links returned by Premiumize. No file bytes, download redirects, or range requests pass through this Worker.

See [technical documentation](spec/proxy.md) for the architecture and implementation contract, and [GitHub Actions deployment](#github-actions-deployment) for automated hosting.

## Behavior

- `GET /api/shop/sections` and `GET /` return the same Cyberfoil sections JSON, including in legacy mode.
- HTTP Basic **username = folder path**, **password = Premiumize API key**.
- For example, `Games/Switch` selects that folder relative to the storage root. `/` explicitly selects the root. One optional leading or trailing slash is accepted. Folder names retain case, spaces and Unicode; do not URL-encode the username yourself.
- Empty usernames, dot segments, repeated interior slashes, backslashes and control characters are rejected. A colon cannot be part of a Basic-auth username.
- Only files directly inside the selected folder are included. Subfolders are ignored, without recursion.
- Supported extensions: `.nsp`, `.nsz`, `.xci`, `.xcz`, case-insensitively. Other files are ignored.
- Each item contains only `name`, `size` in bytes and `url`. Filenames, API order, 64-bit file sizes and signed download URLs are preserved. Missing or invalid metadata for an eligible file fails the catalog instead of returning a broken item.
- There is no title-ID parsing, cover service, metadata database, save sync, upload API or frontend. Unsupported paths return `404`; non-GET methods on catalog paths return `405`.

Every authenticated catalog request makes exactly one request to:

```text
GET https://www.premiumize.me/api/folder/list?path=<encoded-folder>
Authorization: Bearer <api-key>
```

For the root folder, the `path` parameter is omitted. Download links come from the documented `link` field, not from HTML directory scraping or per-file API calls. An empty folder produces a successful empty section. API error envelopes are checked even when Premiumize returns HTTP 200.

## Setup

Requirements: Rust 1.98.1 (the CI-tested toolchain) or a compatible newer stable release, Node.js 22 or newer, npm, a Cloudflare Workers Free account, and Premiumize cloud access. Development and mocked tests do not need either account.

```sh
rustup target add wasm32-unknown-unknown
cargo install worker-build --version 0.8.5 --locked
npm ci
npm test
npm run dev
```

`worker-build` must be on `PATH`. The first build downloads matching wasm-bindgen and wasm-opt tooling. `cargo run` is not used: the application runs as a Wasm Worker, not a native HTTP server.

Wrangler binds to `127.0.0.1` and prints the local URL, normally `http://127.0.0.1:8787`. Use `npm run dev -- --port 8788` when that port is already occupied. The local server calls the real Premiumize API when you supply credentials; only the test suite mocks Premiumize. Use HTTP only for loopback development, and HTTPS for deployed clients.

Node dependencies are development tooling only. Wrangler and its paired Miniflare release are pinned. The `sharp` override applies the patched `0.35.4` release to Miniflare's image dependency; the shop does not use image processing. Recheck the override and `npm audit` when upgrading Miniflare.

### Local Credentials

Keep local testing credentials in an untracked `.env` file. [.env.example](.env.example) contains only the variable names and a sample directory:

```dotenv
PREMIUMIZE_API_KEY=
PREMIUMIZE_DIRECTORY=switch
```

Set the real key locally. Never commit it, put it in an issue, or paste it into logs. [.gitignore](.gitignore) excludes `.env`, environment-specific variants, `.dev.vars` and `*.local.json`; only the placeholder example is allowed. Ignore rules do not remove a file that was already committed. Rotate a key if it has been exposed.

These two variables are **local client-test inputs**, not Worker configuration. The Rust handler does not read environment bindings or use fallback credentials: every catalog request still requires Basic authentication. No Premiumize secrets need to be configured in GitHub or Cloudflare.

Wrangler can load `.env` values into local development bindings even though this Worker ignores them. In a POSIX shell, disable that unnecessary loading before running local development, builds or tests:

```sh
export CLOUDFLARE_LOAD_DEV_VARS_FROM_DOT_ENV=false
```

This does not prevent the explicit Node `--env-file` command below from reading your test credentials. It also does not override a separately created `.dev.vars` file; this project does not need that file.

With the local Worker running, this command uses `.env` to request a catalog and prints only its item count. It contacts the real Premiumize API through the Worker, but does not download any files:

```sh
node --env-file=.env --input-type=module <<'JS'
const key = process.env.PREMIUMIZE_API_KEY;
const directory = process.env.PREMIUMIZE_DIRECTORY;
if (!key || !directory) {
  console.error("Set PREMIUMIZE_API_KEY and PREMIUMIZE_DIRECTORY locally.");
  process.exit(1);
}
try {
  const response = await fetch("http://127.0.0.1:8787/api/shop/sections", {
    headers: { Authorization: `Basic ${Buffer.from(`${directory}:${key}`).toString("base64")}` },
    redirect: "error",
    signal: AbortSignal.timeout(25_000),
  });
  if (!response.ok) {
    console.error(`Catalog request failed (HTTP ${response.status}).`);
    process.exitCode = 1;
  } else {
    const catalog = await response.json();
    const count = catalog.sections.reduce((total, section) => total + section.items.length, 0);
    console.log(`Catalog loaded: ${count} files.`);
  }
} catch {
  console.error("Catalog request failed; check the local Worker and connection.");
  process.exitCode = 1;
}
JS
```

## Deploy

Remain on **Workers Free**. No custom domain, storage binding, server secret or paid service is required. The user's Premiumize key arrives with each request; do not put it into Wrangler configuration.

```sh
npx wrangler login
npm run build
npm run deploy
```

`npm run build` is a deployment dry run and does not publish anything. `npm run deploy` does publish the Worker to your account. Wrangler prints its `https://chfp.<subdomain>.workers.dev` URL. The worker name is configured in [wrangler.toml](wrangler.toml).

Changing the Wrangler worker name creates a separate Cloudflare Worker rather than renaming the existing deployment in place. After deploying `chfp`, verify the new URL and update Cyberfoil. Manage the old `cyberfoil-http-file-proxy` Worker separately.

### GitHub Actions Deployment

The included [workflow](.github/workflows/deploy.yml) validates pull requests and pushes to `main`. A successful push to `main`, or a manual run selected on `main`, then deploys to Cloudflare. Pull requests and manual runs on other branches only validate. Deployment is not enabled merely by creating local credentials.

1. **Publish this project as a GitHub repository.** The project files, including `Cargo.toml`, `Cargo.lock`, `package.json`, `package-lock.json`, `wrangler.toml` and `.github/workflows/deploy.yml`, must be at that repository's root. Enable Actions in the repository settings. Never include `.env` or local credential files. If your deployment branch is not `main`, update both the workflow's push trigger and its deployment condition.
2. **Prepare Cloudflare Workers Free.** Open **Workers & Pages** in the Cloudflare dashboard and configure your account's `workers.dev` subdomain if prompted. Find your [Cloudflare account ID](https://developers.cloudflare.com/fundamentals/account/find-account-and-zone-ids/). Do not use a zone ID. No paid Worker plan or custom domain is needed.
3. **Create a deployment API token.** Follow Cloudflare's [GitHub Actions authentication guide](https://developers.cloudflare.com/workers/ci-cd/external-cicd/github-actions/). Start with the **Edit Cloudflare Workers** token template and restrict account resources to the account that will host this Worker. This project needs Worker script deployment permission (`Account / Workers Scripts / Edit`); it does not configure custom-domain routes, KV or other storage. Remove unrelated resource permissions when tailoring the template. Use an API token, not the Global API Key, and store the value directly in GitHub rather than source control.
4. **Create the GitHub environment.** Go to **Settings > Environments > New environment**, name it exactly `production`, and restrict deployment branches to `main`. Add these two **environment secrets**:

  | Secret | Value |
  | --- | --- |
  | `CLOUDFLARE_API_TOKEN` | The scoped Cloudflare deployment token |
  | `CLOUDFLARE_ACCOUNT_ID` | The Cloudflare account ID |

  Do not add `PREMIUMIZE_API_KEY` or `PREMIUMIZE_DIRECTORY`. Required reviewers can protect production deploys if your GitHub plan/repository visibility supports that feature. The GitHub environment is not a Wrangler `--env production` configuration; this workflow deploys the default Worker from `wrangler.toml`.
5. **Run the workflow.** Commit and push the non-secret project files to `main`, or open **Actions > Test and Deploy Worker > Run workflow** and select `main` after the workflow is on your default branch. Check **Validate Worker** first, approve the `production` deployment if required, and inspect **Deploy Production** for the published `workers.dev` URL. Review changes before merging: merged build scripts run with deployment credentials in the deployment step.
6. **Connect Cyberfoil.** Use the URL from the deployment log with the profile below. Your folder name and Premiumize key still belong in the client's username and password fields.

The validation job uses Rust 1.98.1, Node 22, locked npm/Cargo dependencies, `worker-build` 0.8.5, format checks, Rust tests, Clippy, a Wasm check, and the compiled-Worker integration tests. It receives no deployment secrets. The deployment job starts only after validation passes, rebuilds the same checked-out revision and runs the pinned Wrangler CLI via `npm run deploy`. Production deploys are serialized without canceling an in-progress deployment. Checkout credentials are not persisted; the GitHub token has read-only repository access.

Cloudflare hosting and GitHub Actions have separate allowances. Standard hosted runners are generally free for public repositories; private repositories consume the included Actions minutes for your GitHub plan. This workflow does not request paid Cloudflare services. Avoid also enabling Cloudflare's Git-based automatic builds for the same Worker unless you intentionally want two deployment pipelines.

### Deployment Troubleshooting

| Symptom | Check |
| --- | --- |
| No workflow appears | Workflow file is committed on the default branch; Actions is enabled |
| Tests pass but deployment is skipped | The run must be a push or manual dispatch on `main`, not a pull request |
| Deployment waits for approval | Approve the `production` environment deployment using an authorized reviewer |
| Missing-secret error | Both secrets exist in the `production` environment with exact names |
| Cloudflare authentication/authorization error | Token is valid, has Worker script edit permission, and is scoped to the same account ID |
| Worker URL unavailable | Check `workers.dev` onboarding, `workers_dev = true`, and the URL printed by Wrangler |
| `401` when opening the deployed URL | Expected without Cyberfoil/HTTP Basic credentials; not a deployment failure |

For rollback, select a known-good version in the Cloudflare Worker's deployment history, or revert the bad change through a reviewed commit on `main` so the workflow redeploys it. GitHub secrets and token rotation are managed in account settings, not this repository.

## Cyberfoil Profile

Use the deployed host, HTTPS, port 443 and an empty shop URL path. The storage folder belongs in **username**, not in the shop URL path. Replace the example password locally; never commit the real profile.

```json
{
  "shop": {
    "protocol": "https",
    "host": "chfp.<subdomain>.workers.dev",
    "path": "",
    "port": 443,
    "username": "Games/Switch",
    "password": "YOUR_PREMIUMIZE_API_KEY",
    "title": "Premiumize",
    "favourite": false
  }
}
```

## Free-Plan Budget

Cloudflare's [limits](https://developers.cloudflare.com/workers/platform/limits/) and [pricing](https://developers.cloudflare.com/workers/platform/pricing/), checked September 9, 2026:

| Resource | Workers Free | This Worker |
| --- | --- | --- |
| Inbound requests | 100,000 per day per account | One invocation per catalog or other incoming request; direct downloads do not invoke it |
| CPU | 10 ms per invocation | Minimal typed JSON conversion; network waiting does not count as CPU |
| Subrequests | 50 per invocation | One API fetch; no internal retries or redirect following |
| Memory | 128 MB per isolate, shared across concurrent requests | Upstream catalog body limited to 4 MiB, including responses without Content-Length |

The complete upstream fetch, including reading its body, has a 20-second deadline, below Cyberfoil's documented 30-second catalog timeout. Client retries remain Cyberfoil's responsibility. No KV, D1, R2, Durable Objects, scheduled jobs or Cache API are used. Catalogs and errors use `Cache-Control: private, no-store`; outbound API caching is disabled. There is no persistent credential or catalog storage.

The 1,000-file test fixture, with long filenames and signed URLs, produces approximately 375 KB of JSON. A local release-Wasm inspector run measured about 7.4 ms of sampled active execution per warmed request, versus about 8.6 ms before removing redundant JSON buffering. The deployment dry run reported approximately 556 KiB uncompressed and 205 KiB gzipped. These are local observations, **not a guarantee of deployed CPU accounting, cold-start performance or a supported maximum folder size**. The 4 MiB input bound is a memory safeguard, not a promise that every such listing fits the CPU limit.

Measure your actual catalog in the deployed Worker's metrics. If CPU limits are exceeded, select a smaller folder and inspect CPU usage before adding caching or upgrading plans. The request allowance is account-wide, includes rejected requests, and can be exhausted by public traffic. A Free-plan Worker becomes unavailable when its quota is exceeded; this project does not upgrade your plan. Premiumize subscription and traffic limits are separate from Cloudflare hosting.

## Errors

Errors contain a stable English `error` field. Upstream bodies, raw exceptions and API keys are not returned.

| HTTP status | Meaning |
| --- | --- |
| `400` | Invalid folder syntax or Premiumize rejected the folder request |
| `401` | Missing, malformed or invalid credentials; includes a Basic challenge |
| `403` | Premiumize denied access |
| `404` | Folder not found, or the requested route is not implemented |
| `405` | Unsupported method; catalog routes allow GET only |
| `429` | Premiumize rate, account or service quota reached |
| `502` | Network failure, unexpected redirect, invalid API data/link or oversized listing |
| `503` | Premiumize unavailable |
| `504` | Upstream deadline or upstream timeout |

A syntactically valid upstream `Retry-After` is preserved for quota/unavailability errors. No expiration or reset time is guessed. Cloudflare platform failures may return their own response rather than this JSON format.

## Security and Link Lifetime

- The folder username is a **view selector, not an authorization boundary**. The API key provides account-wide Premiumize access; anyone holding it can choose another folder or call Premiumize directly.
- The fixed Premiumize API endpoint receives the key in a Bearer header, never a query parameter. Upstream redirects are rejected to avoid forwarding this header elsewhere. The Worker accepts no caller-selected upstream URL.
- Catalog links must be absolute HTTPS URLs without embedded username/password or literal whitespace. Their signed tokens are sensitive: do not share catalog output, local profiles or diagnostic headers. The Worker trusts the download destinations returned by Premiumize, does not probe them, and does not restrict them to one particular CDN hostname.
- According to the supplied Cyberfoil documentation, **Cyberfoil forwards Basic credentials to download URLs** and disables certificate verification for shop requests. Those client behaviors cannot be changed by this shop-only Worker. Use a trusted client/network and understand that Premiumize/CDN download endpoints can receive the full API key in the client's Basic header. HTTPS configuration alone does not restore a client's disabled certificate verification.
- `dir.premiumize.me/<api-key>/...` URLs are intentionally not generated: they place the full account key into URL paths. This implementation uses Premiumize's returned file links instead.
- Premiumize link expiration or IP binding is not specified in the supplied documentation. No cache reduces stale-link risk but cannot guarantee a link generated from a Cloudflare IP will work later from the console. Refresh the shop to obtain fresh links. If direct-link compatibility fails, investigate a shop-only alternative rather than introducing download proxying.
- Application code does not log requests, credentials or upstream failures. Persistent Workers observability is disabled in Wrangler configuration. Cloudflare and Premiumize still process the requests; do not assume an absence of provider-side logging or enable verbose tracing with real credentials casually.

## Verification

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo check --target wasm32-unknown-unknown --locked
npm ci
npm audit
npm test
npm run profile
```

`npm test` builds the actual JS/Wasm bundle and tests it in Miniflare with intercepted outbound requests. It covers authentication, both shop routes, path encoding, credential isolation, direct URLs, large sizes, malformed data, HTTP-200 API errors, redirects, quotas, oversized streams, a 1,000-file listing and the real 20-second body deadline. It never contacts Premiumize. The optional profile test is skipped during normal tests.

`npm run profile` requires an existing build, as produced by `npm test` or `npm run build`. It uses a local inspector and 30 warmed 1,000-file requests. Reported sampled active time excludes idle/program samples and is only a local estimate; inspector memory figures are not a complete measurement of Cloudflare's per-isolate accounting. The profile script uses POSIX environment-variable syntax.

### Live Acceptance Checks

These require your own credentials and console and have not been established by mocked tests. Never paste keys or catalog output into chat.

1. Start the local Worker, then fetch the catalog. Supplying only the username makes curl prompt for the API key instead of putting it into shell history. The output file below is ignored by Git:

   ```sh
   curl --fail-with-body --silent --show-error \
     --user 'Games/Switch' \
     --output catalog.local.json \
     'http://127.0.0.1:8787/api/shop/sections'
   ```

2. Check a known folder, an explicitly selected nested folder and a nonexistent path. Confirm Premiumize resolves `path` as documented and never silently substitutes the root for a missing folder.
3. From the console's network, request a small range from a returned direct URL using `Range: bytes=0-1023` and `Accept-Encoding: identity`. Verify HTTP `206`, the expected `Content-Range`, and exactly 1,024 bytes. Keep the signed URL private. Repeat after a realistic catalog-to-download delay.
4. After deployment, load the catalog and test an installation in Cyberfoil. Verify downloads go directly to Premiumize and produce no download invocations on the Worker. Check actual CPU metrics and cold requests before relying on Free-plan hosting.

The implementation has not been deployed automatically and no real Premiumize account has been accessed during development.

## Protocol References

- [Proxy technical documentation](spec/proxy.md)
- [Cyberfoil shop API](spec/cyberfoil/server_api.md)
- [Cyberfoil network downloads](spec/cyberfoil/network_download.md)
- [Premiumize REST API](spec/premiumize-me/api.md)
- [Premiumize HTTPS directory access](spec/premiumize-me/https-directory.md)