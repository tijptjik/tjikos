# Bundled icons

- Solana (SOL), Arweave (AR): Web3 Icons core 4.0.58, monochrome token collection, MIT. Published SVG strings extracted without executing package code from https://registry.npmjs.org/@web3icons/core/-/core-4.0.58.tgz . Source: https://github.com/0xa3k5/web3icons . All share the library's 24px viewbox and white fill. License: WEB3-LICENSE.
- Codex and OpenRouter: original simplified 1.5px outline drawings. Codex uses the same circle and prompt chevron in the tray and usage widget. OpenRouter uses a clear circular inlet and outlined routing silhouette. The retained original brand asset `tray/chatgpt.svg` is from Lobe Icons, MIT, https://github.com/lobehub/lobe-icons/tree/master/packages/static-svg/icons . License: LOBE-LICENSE.
- Bitcoin: restored circular Simple Icons logo, CC0, https://simpleicons.org/ . License: SIMPLE-ICONS-LICENSE.
- ProIcons: MIT, https://github.com/ProCode-Software/proicons/tree/b8718fbc0a8654f6065cff8b3166ee3b99cd89ca . Bundled SVG subset recolored white; semantic mappings in `qml/IconRegistry.js`. License: PROICONS-LICENSE.
- Circle indicators: original simple SVG geometry.

ProIcons supplies matching UI icons, with Material Symbols Rounded as fallback.
Set `style.iconSet:"material"` to use the font icons throughout. StatusNotifier supplies native
tray artwork, rendered in greyscale at 18px in the comparison preset.
Mullvad region flags use flat 4:3 SVGs from flag-icons 7.6.0, MIT,
https://github.com/lipis/flag-icons . License: FLAG-ICONS-LICENSE. Brand owners retain trademark rights.

The bed icon is original. MorphData.js contains sampled coordinates derived from
the bundled MIT ProIcons and original bed SVG; those same attribution terms apply.

Tray overrides: Google Chrome, Mullvad, Telegram, Spotify and WhatsApp (ZapZap) from
Simple Icons 14.15.0, CC0, https://github.com/simple-icons/simple-icons/tree/14.15.0/icons .
Recolored white; viewboxes and logo geometry preserved. Brand trademarks remain
with their owners. Unknown apps retain their native artwork.

`demo-album.png` is the original synthetic Moonlit Tides album artwork from the
notjik demo fixture (generated 2026-10-25); it contains no private media.

ChatGPT uses the already bundled Lobe Icons OpenAI knot, MIT (LOBE-LICENSE).

The `chatgpt-outline`, `telegram-outline` and `whatsapp-outline` tray icons are
original simplified drawings on 24px viewboxes: Codex is a circle with a prompt chevron, Telegram
is the paper-plane silhouette with no internal strokes, and WhatsApp has a compact
filled handset inside its outlined chat bubble. Outlines use 1.5px rounded strokes.
The comparison preset selects them with `iconOverrides`; original brand assets
remain available. Brand trademarks remain with their owners.

Tailscale panel title artwork: Simple Icons 14.15.0, CC0 (SIMPLE-ICONS-LICENSE), recolored white.
