# Premiumize API Documentation

Source: <https://www.premiumize.me/api>

If you need API support, contact [Premiumize customer service](https://www.premiumize.me/help).

## Table of Contents

- [Authentication](#authentication)
  - [API Key](#api-key)
  - [OAuth 20](#oauth-20)
- [Response Format](#response-format)
- [API Methods](#api-methods)
  - [Account](#account)
  - [Folders](#folders)
  - [Files](#files-items)
  - [Transfers](#transfers)
  - [Cache](#cache)
  - [Services](#services)
  - [ZIP Downloads](#zip-downloads)
- [Error Codes](#error-codes)

## Authentication

Every API endpoint accepts either an API key or an OAuth 2.0 access token. Send credentials as a Bearer token:

```sh
curl 'https://www.premiumize.me/api/account/info' \
  -H 'Authorization: Bearer YOUR_API_KEY'
```

### API Key

Find the API key under [Account](https://www.premiumize.me/account). The `Authorization` header is preferred because it keeps the key out of server logs, browser history, `Referer` headers, and CDN caches.

Legacy forms are still accepted:

- `?apikey=YOUR_API_KEY` in the query string or `apikey=YOUR_API_KEY` in a POST body.
- `?pin=YOUR_API_KEY` or `pin=YOUR_API_KEY`, a legacy alias for `apikey`.
- A session cookie, used only by the website's own JavaScript.

### OAuth 2.0

Register an OAuth client at <https://www.premiumize.me/registerclient>. Authorization is available at `https://www.premiumize.me/authorize`, and token exchange is available at `https://www.premiumize.me/token`. The only scope is `full`.

#### Authorization Code with PKCE

This is the recommended flow for mobile, SPA, CLI, and native applications.

1. Generate a random URL-safe `code_verifier` between 43 and 128 characters. Create `code_challenge` by SHA-256 hashing the verifier and Base64URL-encoding the result without padding.

   ```text
   code_verifier  = base64url(random_bytes(32))
   code_challenge = base64url(sha256(code_verifier))
   ```

2. Redirect the user to:

   ```text
   https://www.premiumize.me/authorize
       ?response_type=code
       &client_id=YOUR_CLIENT_ID
       &redirect_uri=https://your.app/callback
       &state=RANDOM_STRING
       &code_challenge=THE_CHALLENGE
       &code_challenge_method=S256
   ```

3. Verify the returned `state`, then exchange the one-time code:

   ```sh
   curl -X POST 'https://www.premiumize.me/token' \
     -d 'grant_type=authorization_code' \
     -d 'code=THE_CODE' \
     -d 'client_id=YOUR_CLIENT_ID' \
     -d 'redirect_uri=https://your.app/callback' \
     -d 'code_verifier=THE_VERIFIER'
   ```

A denial returns `error=access_denied`. The verifier must be kept private and is never sent in the initial redirect.

#### Device Code

Use this flow for TVs, set-top boxes, and other input-constrained devices.

Request a code pair:

```sh
curl -X POST 'https://www.premiumize.me/token' \
  -d 'response_type=device_code' \
  -d 'client_id=YOUR_CLIENT_ID'
```

Example response:

```json
{
  "verification_uri": "https://www.premiumize.me/device",
  "user_code": "kpwx-3m7r",
  "device_code": "...",
  "expires_in": 600,
  "interval": 5
}
```

Display `verification_uri` and `user_code`, then poll at least every `interval` seconds:

```sh
curl -X POST 'https://www.premiumize.me/token' \
  -d 'grant_type=device_code' \
  -d 'code=THE_DEVICE_CODE' \
  -d 'client_id=YOUR_CLIENT_ID'
```

A successful response contains `access_token`, `token_type`, `expires_in`, and `scope`. Pending and error responses use HTTP 400:

| Error | Meaning | Action |
|---|---|---|
| `authorization_pending` | The user has not entered the code. | Continue polling. |
| `slow_down` | The poll was too soon. | Wait longer; clients may increase the interval. |
| `access_denied` | The user rejected authorization. | Stop and report the denial. |
| `invalid_grant` | The device code is unknown, mismatched, or expired. | Stop and start over if necessary. |

#### Authorization Code

For server-side applications that can keep a client secret safely:

```sh
curl -X POST 'https://www.premiumize.me/token' \
  -d 'grant_type=authorization_code' \
  -d 'code=THE_CODE' \
  -d 'client_id=YOUR_CLIENT_ID' \
  -d 'client_secret=YOUR_CLIENT_SECRET' \
  -d 'redirect_uri=https://your.app/callback'
```

First redirect the user to `/authorize` with `response_type=code`, `client_id`, `redirect_uri`, and a random `state`, then verify `state` before exchanging the code.

#### Resource Owner Password Credentials (Legacy)

This legacy flow is discouraged by OAuth 2.1 and should only be used by trusted first-party applications:

```sh
curl -X POST 'https://www.premiumize.me/token' \
  -d 'grant_type=password' \
  -d 'client_id=YOUR_CLIENT_ID' \
  -d 'client_secret=YOUR_CLIENT_SECRET' \
  -d 'username=USER_EMAIL' \
  -d 'password=USER_PASSWORD'
```

#### Implicit (Legacy)

The implicit flow is deprecated. Prefer PKCE. Existing clients redirect with `response_type=token` and receive the access token in the URL fragment:

```text
https://your.app/callback#access_token=...&token_type=Bearer&expires_in=...&state=...
```

## Response Format

Normal API responses use HTTP 200 and include a `status` field:

```json
{
  "status": "success"
}
```

Errors normally use the same HTTP status and include a stable `code`:

```json
{
  "status": "error",
  "message": "Human-readable description.",
  "code": "error_code_string"
}
```

HTTP 500 is reserved for catastrophic failures and still returns a JSON envelope on `/api/*` paths. Undocumented fields may be legacy or experimental and must not be relied upon.

## API Methods

All paths below are relative to `https://www.premiumize.me`.

### Account

#### `GET /api/account/info`

Returns authenticated account information. No parameters.

```json
{
  "status": "success",
  "customer_id": "1234567",
  "premium_until": 1799999999,
  "limit_used": 0.42,
  "booster_points": 0
}
```

`premium_until` is a Unix timestamp or `null` for free accounts. `limit_used` is a fair-use fraction from 0 to 1. The old `space_used` field is informational only and must not be used for quota enforcement.

### Folders

#### `GET /api/folder/list`

Lists a folder. Optional parameters: `id`, `path`, and `includebreadcrumbs=true`. File entries include `id`, `name`, `type`, `created_at`, `size`, `mime_type`, and `link`; folder entries include the first four fields. Breadcrumbs are ordered from root to the current folder.

#### `POST /api/folder/create`

Parameters: required `name`; optional `parent_id`. Returns `{ "status": "success", "id": "..." }`.

#### `POST /api/folder/rename`

Parameters: `id` and new `name`. Returns a success envelope.

#### `POST /api/folder/delete`

Parameter: `id`. Deletes the folder and its contents.

#### `POST /api/folder/paste`

Moves files and/or folders into target folder `id`. At least one of `files[]` or `folders[]` is required.

#### `GET /api/folder/uploadinfo`

Optional `id` selects the target folder. Returns a single-use upload `token` and `url`:

```sh
curl -X POST "$URL" \
  -F "token=$TOKEN" \
  -F "file=@/path/to/file"
```

#### `GET /api/folder/search`

Parameter: `q`. Searches files and folders across cloud storage. Results use the same file and folder shapes as `/api/folder/list`.

### Files (Items)

#### `GET /api/item/details`

Parameter: file `id`. Returns `id`, `name`, `size`, `created_at`, `folder_id`, `mime_type`, and `link`. Folder IDs are not accepted.

#### `POST /api/item/rename`

Parameters: `id` and new `name`.

#### `POST /api/item/delete`

Parameter: file `id`.

#### `GET /api/item/listall`

Returns every owned file with `id`, `name`, slash-joined `path`, `size`, `created_at`, and `mime_type`.

Deprecated fields throughout the file APIs include `directlink`, `stream_link`, `transcode_status`, checksums, codec and resolution metadata, virus-scan values, and unpackability heuristics. Use `link` and the documented fields only.

### Transfers

#### `POST /api/transfer/create`

Submits a URI or multipart `src` for asynchronous cloud download. Optional parameters are `folder_id` and `password`. A normal response contains `id` and `name`. Container sources (`.dlc`, `.ccf`, or `.rsdf`) return `type: "container"` and extracted links to submit individually.

#### `POST /api/transfer/directdl`

Parameter: `src`. Generates direct links without storing the source. The response contains a `content` array of `{ path, size, link }` entries.

#### `GET /api/transfer/list`

Returns all transfers, newest first. Each transfer contains `id`, `name`, `status`, `progress`, `message`, `folder_id`, and `file_id`. Status values are `queued`, `running`, `finished`, `seeding`, and `error`. Progress is from 0.0 to 1.0.

#### `POST /api/transfer/retry`

Parameter: failed transfer `id`. Re-queues the transfer.

#### `POST /api/transfer/delete`

Parameter: transfer `id`. Deletes the transfer record.

#### `POST /api/transfer/clearfinished`

Removes all finished transfers. No parameters.

### Cache

#### `POST /api/cache/check`

Parameter: `items[]`, a list of links. The response contains parallel arrays `response`, `filename`, and `filesize`, indexed in request order.

The legacy `transcoded` field must not be used.

### Services

#### `GET /api/services/list`

Returns service metadata:

- `cache`: services supported by `/api/cache/check`.
- `directdl`: services supported by `/api/transfer/directdl`.
- `queue`: services supported by `/api/transfer/create`.
- `fairusefactor`: fair-use multiplier by service.
- `aliases`: recognized domain aliases.
- `regexpatterns`: URL-detection patterns.

Aliases and regular expressions are best-effort and are not guaranteed to be complete.

### ZIP Downloads

#### `POST /api/zip/generate`

Parameters: optional `files[]` and `folders[]`; at least one must be supplied. Returns a synchronous download URL:

```json
{
  "status": "success",
  "location": "https://..."
}
```

The filename is encoded in the URL because the server does not set `Content-Disposition`. A single file becomes `file.ext.zip`, a single folder becomes `folder.zip`, and multiple sources receive a generated date-based filename.

## Error Codes

The `code` field is stable and grouped by retry behavior.

### Transient

Retrying may succeed immediately:

| Code | HTTP | Meaning |
|---|---:|---|
| `link_generation_failed` | 200 | Link generation failed temporarily. |
| `transient_error` | 200 | Other unspecified transient failure. |

### Semi-Permanent

Wait, raise quota, or upgrade before retrying:

| Code | HTTP | Meaning |
|---|---:|---|
| `service_down` | 200 | Target service is currently unreachable. |
| `service_limit_reached` | 200 | The account's limit for the service was reached. |
| `account_limit_reached` | 200 | Fair-use points, booster points, or active-job capacity is exhausted. |
| `rate_limit_reached` | 200 | Too many API requests were made too quickly. |
| `semi_permanent_error` | 200 | Other unspecified semi-permanent failure. |

### Permanent

The same request will continue to fail:

| Code | HTTP | Meaning |
|---|---:|---|
| `service_unsupported` | 200 | The target service cannot be processed. |
| `not_found` | 200 | The requested resource does not exist. |
| `authentication_failed` | 200 | Credentials are missing, invalid, or expired. |
| `permission_denied` | 200 | The authenticated account is not allowed to perform the operation. |
| `invalid_request` | 200 | Required parameters are missing or malformed. |
| `permanent_error` | 200 | Other unspecified permanent failure. |

### Unknown

| Code | HTTP | Meaning |
|---|---:|---|
| `unknown_error` | 500 | Network failure, parse error, or uncaught server exception. Back off and retry. |
