# AMRI VPN visual assets

Use only the files in this folder as the current approved visual set.

- `amri-icon.svg` — product icon.
- `background-desktop.svg` — desktop background; use `cover`.
- `background-mobile.svg` — mobile background; use `cover`; it has no fixed visual elements at edges and adapts to different phone aspect ratios.
- `vpn-power-on.svg` — active/connected power button; transparent outside the button.
- `vpn-power-off.svg` — inactive/disconnected power button.
- `settings-button.svg` — transparent settings button; open the local settings screen only.

Do **not** use the earlier raster ON button with the solid dark square background. It was superseded by `vpn-power-on.svg`.

The next implementation chat must wire these files into both Windows and Android UI, retain accessibility labels and never represent the VPN as connected until the transport is genuinely active.
