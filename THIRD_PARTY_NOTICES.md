# Third-party notices

AMRI VPN is proprietary software. The components listed below remain subject to their own licenses.

## sing-box 1.14.1

Project: `sing-box`

Source: https://github.com/SagerNet/sing-box/tree/v1.14.1

Use in AMRI: separately executed Windows transport component. AMRI sends a runtime configuration
through stdin; sing-box does not own AMRI route selection, learning or UI.

License: GNU General Public License v3.0 or later. The unmodified official executable and its
license are packaged separately. The release workflow pins the official archive SHA-256. Source
for the exact packaged version is available at the link above.

## tun2proxy 0.8.3

Project: `tun2proxy`

Source: https://github.com/tun2proxy/tun2proxy

Use in AMRI: Android and Windows userspace packet-forwarding layer between an AMRI-created TUN interface and an already-confirmed local SOCKS transport endpoint. AMRI does not use tun2proxy for route selection, credential storage, or VPN protocol configuration.

License: MIT

Copyright (c) @ssrlive, B. Blechschmidt and contributors

## tproxy-config 7.0.7

Project: `tproxy-config`

Source: https://github.com/ssrlive/tproxy-config

Use in AMRI: Windows route/DNS setup and restoration for the active AMRI TUN generation. It does not own AMRI route selection, credentials, or protocol configuration.

License: MIT

Package author metadata: @ssrlive

## MIT license text

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Wintun 0.14.1 runtime packaging

Project: `Wintun`

Official distribution: https://www.wintun.net/

Use in AMRI: the Windows installer downloads the official signed Wintun 0.14.1 archive, verifies the archive SHA-256, and packages only the official amd64 `wintun.dll` plus the license material from that archive. AMRI does not rebuild or rename an unofficial driver binary as Wintun.

The Wintun source code and the official prebuilt signed binaries have different distribution terms. AMRI keeps the upstream binary license/provenance with the installer and does not claim Wintun as proprietary AMRI code.

Before a public binary release, the complete transitive dependency license inventory must also be generated/reviewed as part of release compliance. This file does not replace that full release audit.
