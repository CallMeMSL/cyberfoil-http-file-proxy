[Skip to content](https://cyberfoil.foo/network.html#main)

[![CyberFoil](https://cyberfoil.foo/logo.png)](https://cyberfoil.foo/index.html)

- [Home](https://cyberfoil.foo/index.html)
- [Features](https://cyberfoil.foo/features.html)
- [Guide](https://cyberfoil.foo/guide.html)
- [Docs](https://cyberfoil.foo/docs.html)
- [Shop](https://cyberfoil.foo/shop.html)
- [Network](https://cyberfoil.foo/network.html)
- [Install](https://cyberfoil.foo/install.html)
- [Credits](https://cyberfoil.foo/credits.html)

# Network & Download

How CyberFoil moves bytes from a source to the installer.

NSP/XCI parsing: [Install pipeline](https://cyberfoil.foo/install.html)  
Shop HTTP API: [Shop](https://cyberfoil.foo/shop.html)

## Contents

- [Connection modes](#connection-modes)
- [URL & path types](#url--path-types)
- [HTTP streaming download](#http-streaming-download)
- [LAN protocol](#lan-protocol---port-2000)
- [USB protocol](#usb-protocol)
- [Local & MTP](#local--mtp)

## Connection Modes

| UI source | Transport | Reader | Source |
|---|---|---|---|
| LAN install | TCP `:2000`, then HTTP range | `HTTPNSP` / `HTTPXCI` | `netInstall.cpp` |
| URL / Google Drive | HTTP(S) range | `HTTPNSP` / `HTTPXCI` | `netInstPage.cpp` |
| eShop | HTTP(S) + shop authentication | `HTTPNSP` / shop XCI stream | `shopInstall.cpp` |
| SD card | Local file | `SDMCNSP` / `SDMCXCI` | `sdInstall.cpp` |
| USB HDD | USB volume file | `SDMCNSP` / `SDMCXCI` | `hddInstall.cpp` |
| NS-USBloader | USB bulk TUC0/TUL0 | `USBNSP` / `USBXCI` | `usbInstall.cpp` |
| MTP | Push to `install/` | `MtpNspStream` | `mtp_install.cpp` |

Target storage:

- `0` = SD card (`NcmStorageId_SdCard`)
- Any other value = internal storage (`NcmStorageId_BuiltInUser`)

## URL & Path Types

| Kind | Example | Handling |
|---|---|---|
| HTTP(S) direct | `https://host/a.nsp` | Uses curl with HTTP Range requests; expects `206 Partial Content` |
| Name fragment | `url#Display%20Name` | Fragment is used as a label only |
| Google Drive | File ID from the keyboard | `drive/v3/files/{id}?key=…&alt=media` |
| Shop relative URL | `/files/x.nsp` | Joined to the shop base URL |
| JBOD | `jbod:{chunk}/{url}/…` | Virtual file assembled from multiple URLs |
| Local path | SD / USB filesystem path | No HTTP is used |
| LAN list | URLs over TCP `:2000` | Newline-separated URL list |

Relevant source files:

```text
network_util.cpp
util/util.cpp
shopInstall.cpp
```

## HTTP Streaming Download

Remote installs stream directly into NCM placeholders. The complete file is not saved to the SD card first.

- `Range: offset-(offset+size-1)` is used for HTTP range requests.
- The server must return HTTP `206 Partial Content`.
- `Accept-Encoding: identity` is sent.
- Each segment is retried three times with a two-second delay.
- After all retries are exhausted, the user is shown a retry dialog.
- NSP versus XCI detection:
  - Bytes `0x100` through `0x103` equal `HEAD` → XCI
  - Otherwise → NSP

Install requests may include Tinfoil headers such as `Theme`, `UID`, `HAUTH`, and `UAUTH`, as well as HTTP Basic authentication when configured.

Relevant source files:

```text
network_util.cpp
curl.cpp
netInstall.cpp
```

## LAN Protocol - Port 2000

1. The client connects to `<switch-ip>:2000`.
2. The client receives a `u32` size followed by a payload containing newline-separated URLs.
3. The maximum payload size is approximately 262 KiB.
4. Each URL is installed using HTTP range requests.
5. When finished, the client sends a one-byte `0x00` acknowledgement.
6. The client sends an HTTP `DROP` request to the host.

The `Y` button on the network screen opens the manual URL or Google Drive ID input.

Relevant source:

```text
netInstall.cpp
```

The port is defined as:

```cpp
REMOTE_INSTALL_PORT = 2000
```

## USB Protocol

- **TUL0** - Title list, identified by magic `0x304C5554`
- **TUC0** - File range commands with 8 MiB bulk reads

An XCI is selected when the title name ends with `xc`.

Relevant source files:

```text
usb_util.cpp
usbInstall.cpp
```

## Local & MTP

Supported file extensions:

```text
.nsp
.nsz
.xci
.xcz
```

SD card and USB HDD installs use sequential reads in 4 MiB blocks.

MTP performs incremental parsing while the file is uploaded to:

```text
install/
```

Relevant source files:

```text
sdInstall.cpp
mtp_install.cpp
```

---

CyberFoil · luketanti

- [GitHub](https://github.com/luketanti/CyberFoil)
- [Releases](https://github.com/luketanti/CyberFoil/releases)