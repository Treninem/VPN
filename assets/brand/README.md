# AMRI VPN visual assets

Use only the files in this folder as the current approved visual set. `assets/brand` is the single source of truth for AMRI artwork. Do not redraw, recolor, export a visually different duplicate, or add manually maintained copies under Windows or Android resource folders.

- `amri-icon.png` — canonical AMRI VPN product/app icon. This is the approved premium blue shield/A/three-arrows artwork. Windows embeds this exact file. Android copies this same file at build time to the generated `amri_app_icon.png` resource; it must never use a separately redrawn icon.
- `background-desktop.svg` — Windows desktop background; render with cover-style scaling.
- `background-mobile.svg` — Android mobile background; render with cover-style scaling. It has no fixed visual elements at the edges and adapts to different phone aspect ratios.
- `vpn-power-on.svg` — active/connected power button; transparent outside the button. Used only when the transport/service state is genuinely active.
- `vpn-power-off.svg` — inactive/disconnected/preparing power state; transparent outside the button.
- `settings-button.svg` — transparent settings button. Windows opens the settings page; Android toggles its local settings panel.
- `language-button.svg` — transparent language selector. Language changes locally and does not modify VPN subscriptions.
- `add-button.svg` — add/import action. Windows currently uses it for manual node/subscription import.
- `edit-button.svg` — edit the selected Windows node locally. The credential-bearing URI is exposed in the editor only after the explicit edit action.
- `delete-button.svg` — delete the selected Windows node with mandatory confirmation.
- `copy-button.svg` — copy a non-secret value. Windows copies only the node fingerprint; never silently copy credentials/keys.
- `info-button.svg` — show safe node information (protocol, host, port, fingerprint) without the raw credential URI.
- `more-button.svg` — open/close secondary actions for the selected Windows node.
- `close-button.svg` — cancel Windows node editing without saving and clear the temporary credential-bearing editor value.
- `back-button.svg` — return to the previous Windows page using local navigation history.
- `refresh-button.svg` — approved visual reserved for a real refresh operation. **Do not display it as an active control until the client has an implemented subscription/source refresh backend.**

Do **not** use the earlier raster ON button with the solid dark square background. It was superseded by `vpn-power-on.svg`.

## Byte-for-byte integrity

`asset-blobs.lock` records the exact Git blob ID of every approved image. CI runs `scripts/verify_visual_assets.py` and fails if an approved file changes byte-for-byte, disappears, gains a platform-specific duplicate, or stops being referenced from the canonical path.

When the user intentionally approves a replacement image, review that exact final file first, commit that file, then update only its corresponding entry in `asset-blobs.lock` to the new value from `git hash-object assets/brand/<file>`. Never update the lock merely to silence CI for an unreviewed visual change.

## Platform wiring

Windows loads its approved visuals directly from `assets/brand` with compile-time image includes. Do not create a second Windows artwork directory for these files.

Android build tasks generate only build-output resources from the canonical files in `assets/brand`. Those generated resources are implementation copies, not alternate artwork and must not be committed as independently maintained images. The Android manifest points at the generated copy of the canonical `amri-icon.png`.

Not every approved icon must appear on every platform. A button should be wired only when the corresponding real action exists. Do not add decorative or non-functional controls merely to use an available asset.

All interactive images must retain accessibility labels. Never represent the VPN as connected until the transport/service state is genuinely active.
