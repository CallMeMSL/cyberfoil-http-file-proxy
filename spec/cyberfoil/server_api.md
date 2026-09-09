```markdown
# Shop Server API

Reference for running or integrating a shop backend compatible with CyberFoil.  
Works best with **AeroFoil**. Derived from `shopInstall.cpp` and `save_sync.cpp`.

See also: [Network & download](https://cyberfoil.foo/network.html) · [Install pipeline](https://cyberfoil.foo/install.html)

## Contents

- [Connecting](#connecting-from-cyberfoil)
- [Catalog fetch](#how-cyberfoil-loads-a-catalog)
- [Endpoints](#http-endpoints)
- [Sections JSON](#modern-response-apis hopsections)
- [Item fields](#item-fields)
- [Legacy / Tinfoil](#legacy--tinfoil-mode)
- [Headers & auth](#request-headers)
- [Save sync](#save-sync-apisaves)
- [Client config](#related-client-settings)
- [Server checklist](#minimal-compatible-server)

## Connecting from CyberFoil

Set the shop URL in Settings or save a profile under:

```text
sdmc:/switch/CyberFoil/shops/
```

- **Default HTTP port:** `8465`
- **HTTPS default port:** `443`
- **Example URL:** `http://192.168.1.2:8465`
- **Authentication:** Optional HTTP Basic authentication using `shopUser` / `shopPass` in the config, or credentials defined per profile in `shops/*.json`
- Trailing slashes are stripped.
- Bare hostnames are prefixed with `http://`.

### Shop profile example

```json
{
  "shop": {
    "protocol": "http",
    "host": "192.168.1.2",
    "path": "",
    "port": 8465,
    "username": "user",
    "password": "pass",
    "title": "My LAN shop",
    "favourite": false
  }
}
```

## How CyberFoil Loads a Catalog

1. If `shopLegacyMode` is enabled, CyberFoil performs a `GET` request against the shop root only.
2. Otherwise, it requests `GET /api/shop/sections`.
3. On `404` or network failure, CyberFoil falls back to a root `GET` request for a legacy JSON or TINFOIL binary response.
4. After loading, an optional MOTD is read from the root JSON field `success`. This is skipped if the response body is TINFOIL.
5. Search is performed client-side and filters item names in the current section. There is no search HTTP API.

## HTTP Endpoints

All catalog requests use `GET`. File installs use `GET` on each item's `url`, with Basic authentication forwarded.

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/api/shop/sections` | Modern catalog endpoint |
| `GET` | `/` | Legacy catalog, MOTD, or TINFOIL payload |
| `GET` | `/api/shop/icon/{TITLEID}` | Icon for a 16-character uppercase hexadecimal title ID, for example `01007EF00011E000` |
| `GET` | `{item.url}` | Download an NSP, NSZ, XCI, or XCZ file for installation |
| `GET` | `/api/saves/list` | List remote save backups in non-legacy mode |
| `POST` | `/api/saves/upload/{TITLEID}` | Upload a save ZIP archive using multipart form data |
| `GET` | `/api/saves/download/{TITLEID}.zip` | Download the latest save |
| `GET` | `/api/saves/download/{TITLEID}/{saveId}.zip` | Download a specific save version |
| `DELETE` | `/api/saves/delete/{TITLEID}` | Delete saves for a title |
| `DELETE` | `/api/saves/delete/{TITLEID}/{saveId}` | Delete one save version |

Shop fetches use a 30-second timeout and up to four retries for HTTP status codes `408`, `429`, and `5xx`.

SSL certificate verification is disabled for shop requests.

## Modern Response: `/api/shop/sections`

The endpoint should return JSON using the `Content-Type: application/json` header.

### Error response

```json
{
  "error": "Invalid credentials"
}
```

### Success response

```json
{
  "sections": [
    {
      "id": "base",
      "title": "Games",
      "items": [
        {
          "name": "Example Game",
          "url": "/files/example.nsp",
          "size": 1234567890,
          "title_id": "01007EF00011E000",
          "app_version": 65536,
          "app_type": "base",
          "icon_url": "/api/shop/icon/01007EF00011E000",
          "release_date": 20240115
        }
      ]
    }
  ],
  "success": "Welcome - shown as message of the day"
}
```

The following section IDs have special meaning on the client:

- `all`
- `updates` or `update`
- `dlc`
- `installed` for local content
- `saves` or `save`

## Item Fields

| Field | Aliases | Notes |
|---|---|---|
| `url` | None | Required. Relative paths are resolved against the shop base URL. If `name` is absent, the URL fragment is used as the display name. |
| `name` | None | Display title |
| `size` | None | File size in bytes |
| `title_id` | None | A 16-character hexadecimal or decimal title ID |
| `app_version` | None | Version number |
| `app_type` | None | `base`, `upd`, `update`, `patch`, `dlc`, `addon`, or numeric values `0`, `1`, and `2` |
| `icon_url` | `iconUrl` | If omitted and `title_id` is set, the client automatically uses `/api/shop/icon/{ID}` |
| `release_date` | `releaseDate`, `date` | Integer in `YYYYMMDD` format |

After parsing, CyberFoil may prompt the user to include related updates and DLC for selected base titles.

## Legacy / Tinfoil Mode

Enable `shopLegacyMode` in Settings to use Tinfoil compatibility mode.

This changes the client behavior:

- Only the shop root is requested. `/api/shop/sections` is not used.
- The `User-Agent` header is empty.
- Save synchronization is disabled.
- Legacy headers report version `20.0.2`.

### Legacy JSON at the Root

The root response must provide at least one of the following:

- `sections` - Same structure as the modern API
- `files` or `paths` - An array of objects or a map of `"Display Name": "path_or_url"`
- `directories` - URLs fetched recursively for nested manifests

Supported per-entry URL keys:

- `url`
- `path`
- `file`
- `download_url`
- `downloadUrl`
- A plain string value

### TINFOIL Binary

The response body starts with the magic string `TINFOIL`. An optional BOM or whitespace prefix is allowed.

The payload may be AES-encrypted and/or compressed using zstd or zlib. The decrypted payload must contain JSON.

Encrypted shops without the required library blob are rejected with an error.

```json
{
  "files": {
    "Game [01007EF00011E000][base]": "/nsp/game.nsp"
  }
}
```

## Request Headers

### HTTP Basic Authentication

When a username or password is configured, requests include the standard header:

```http
Authorization: Basic <base64-credentials>
```

### User-Agent for Non-Legacy Shop Fetches

| `httpUserAgentMode` | Sent value |
|---|---|
| `default` | `cyberfoil` |
| `chrome`, `safari`, `firefox` | Browser preset string |
| `tinfoil` | Empty string |
| `custom` | Value from `httpUserAgent` in the config |

### Tinfoil Legacy Headers

When legacy authentication support is available in the build, shop requests may also include:

- `Theme:` 64 zero digits
- `UID:` Derived from the console CID
- `Version:` and `Revision:` From the app version
- `Language:` For example, `en` or `fr`
- `HAUTH:` HMAC-like token derived from the shop URL
- `UAUTH:` Token derived from the URL and credentials

HTTP `401` and `403` responses, as well as HTML login pages, are treated as authentication failures.

A redirect to a URL containing `/login` also causes login to fail.

## Save Sync: `/api/saves/*`

Save synchronization requires non-legacy shop mode. It uses the same shop URL and Basic authentication and is compatible with AeroFoil-style servers.

### List: `GET /api/saves/list`

The endpoint may return a JSON array directly or an object containing a `saves` array.

```json
{
  "saves": [
    {
      "title_id": "01007EF00011E000",
      "name": "Example Game",
      "save_id": "abc123",
      "note": "Before final boss",
      "created_at": "2024-01-15T12:00:00Z",
      "created_ts": 1705312800,
      "size": 1048576,
      "download_url": "/api/saves/download/01007EF00011E000/abc123.zip"
    }
  ]
}
```

Supported field aliases include:

- `titleId`
- `saveId`
- `save_note`
- `downloadUrl`

### Upload: `POST /api/saves/upload/{TITLEID}`

Use multipart form data with the following fields:

- `file` - ZIP archive
- `title_id` or `application_id` - 16-character hexadecimal title ID
- `note` - Required by the UI for bulk backups

The server should return any HTTP `2xx` status code on success.

### Download and Delete

Downloads use either a constructed URL or the provided `download_url`.

Delete endpoints remove saves by title or by specific `save_id`.

## Related Client Settings

| Key | Default | Effect |
|---|---:|---|
| `shopHideInstalled` | `true` | Hide already-installed titles in the shop UI |
| `shopHideInstalledSection` | `true` | Hide the synthetic Installed section |
| `shopAllBaseOnly` | `true` | Show base titles only in the All section |
| `shopLegacyMode` | `false` | Enable Tinfoil compatibility mode |
| `shopStartGridMode` | `false` | Open the shop in grid view |

### Icon Cache

Icons are cached at:

```text
sdmc:/switch/CyberFoil/shop_icons/
```

## Minimal Compatible Server

1. Listen on HTTP port `8465` or HTTPS port `443`.
2. Implement `GET /api/shop/sections` and return `{ "sections": [ ... ] }`.
3. Serve files at the URLs referenced by items and support relative paths.
4. Optionally implement `GET /api/shop/icon/{TITLEID}` and provide a root-level `success` MOTD string.
5. For save synchronization, implement `/api/saves/list`, `/api/saves/upload`, `/api/saves/download`, and `/api/saves/delete`.
6. For older Tinfoil shops, support a root JSON response or TINFOIL binary payload, plus legacy headers when encrypted.
