# Third-party notices

AMRI VPN is proprietary software. The components listed below remain subject to their own licenses.

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

## Wintun runtime packaging

The official Wintun prebuilt runtime is not committed to this repository. Windows release packaging requires the official signed `wintun.dll` matching the target architecture. The release artifact must retain Wintun's official binary license/provenance distributed with the official package. Source-code licensing for Wintun is separate from the permissive license covering its official prebuilt binaries and must not be conflated with AMRI's proprietary source.

Before a public binary release, the complete transitive dependency license inventory must also be generated/reviewed as part of release compliance. This file does not replace that full release audit.
