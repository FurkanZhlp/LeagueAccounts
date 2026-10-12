// Pinned client art, fonts and silent motion. Runtime never contacts a CDN.
// --fetch restores the bundle; without it, verify every committed asset hash.
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = resolve(root, "ui/public/league");
const manifestPath = resolve(output, "manifest.json");
const base = "https://raw.communitydragon.org/16.20/plugins/";
const shared = "rcp-fe-lol-static-assets/global/default/";
const typekit = "rcp-fe-lol-typekit/global/default/fonts/";
const uikit = "rcp-fe-lol-uikit/global/default/images/";
const tiers = ["iron", "bronze", "silver", "gold", "platinum", "emerald", "diamond", "master", "grandmaster", "challenger"];
const assets = [
  ...tiers.map(tier => [`ranks/${tier}.png`, `${shared}ranked-emblem/emblem-${tier}.png`]),
  ["ranks/unranked.png", "rcp-fe-lol-shared-components/global/default/unranked.png"],
  ...[...tiers, "unranked"].flatMap(tier => [
    [`crests/${tier}.svg`, `${shared}ranked-mini-crests/${tier}.svg`],
    [`crests/${tier}-tft.svg`, `${shared}ranked-mini-crests/${tier === "unranked" ? "unranked-tft" : `${tier}_tft`}.svg`],
  ]),
  ["fonts/beaufort-medium.otf", `${typekit}beaufort/beaufortforlol-medium.otf`],
  ["fonts/beaufort-regular.otf", `${typekit}beaufort/beaufortforlol-regular.otf`],
  ["fonts/beaufort-bold.otf", `${typekit}beaufort/beaufortforlol-bold.otf`],
  ["fonts/spiegel-regular.otf", `${typekit}spiegel/spiegel-regular.otf`],
  ["fonts/spiegel-semibold.otf", `${typekit}spiegel/spiegel-semibold.otf`],
  ["backgrounds/hextech.jpg", `${shared}lcu-alpha-backdrop.jpg`],
  ["backgrounds/collection.jpg", `${shared}npe-rewards-background.jpg`],
  ["icons/empty-poro.svg", `${shared}icon-search-empty-poro.svg`],
  ["icons/profile.svg", `${shared}nav-icon-profile.svg`],
  ["icons/lol.png", `${shared}lol_icon.png`],
  ["icons/tft.svg", "rcp-fe-lol-parties/global/default/tft-game-select-icon-default.svg"],
  ...["sub-border-primary-horizontal", "sub-border-secondary-horizontal", "sub-border-primary-vertical", "sub-border-secondary-vertical", "frame-button-close-top-down", "x-icon", "title_divider"].map(name => [`frame/${name}.png`, `${uikit}${name}.png`]),
  ...["checkbox-spritesheet", "up-down-arrow", "up-down-arrow-locked", "dropdown-check", "nav-pointer", "nav-highlight", "sheen"].map(name => [`controls/${name}.png`, `${uikit}${name}.png`]),
  ...["icon_copy", "icon_delete", "icon_edit", "icon_import", "icon_export", "icon_plus", "icon_remove", "icon_settings", "icon_next", "refresh", "external-link-mask"].map(name => [`icons/${name}.png`, `${uikit}${name}.png`]),
  ...["ellipses", "preview", "info-icon", "attention-info"].map(name => [`icons/${name}.svg`, `${uikit}${name}.svg`]),
  ["icons/search.png", `${shared}uikit/magnifying-glass-icon.png`],
  ["icons/check.svg", `${shared}checkmark.svg`],
  ["icons/warning.svg", `${shared}warning_icon.svg`],
  ["icons/clock.svg", `${shared}hextech-ui-icons/clock.svg`],
  ["controls/spinner.png", `${shared}spinner.png`],
  ["controls/radio.png", "rcp-fe-lol-parties/global/default/btn_icon.png"],
  ["icons/lock.svg", `${shared}challenges-shared/icon-lock.svg`],
  ...["control-close", "control-hide"].map(name => [`icons/${name}.png`, `rcp-fe-lol-navigation/global/default/${name}.png`]),
  ...["tooltip-caret", "tooltip-sub-border-horizontal"].map(name => [`frame/${name}.png`, `${shared}uikit/tooltip/${name}.png`]),
  ["frame/tooltip-system-caret.png", `${uikit}tooltip-system-caret.png`],
];
const hash = buffer => createHash("sha256").update(buffer).digest("hex");

if (process.argv.includes("--fetch")) {
  let previous;
  try { previous = JSON.parse(await readFile(manifestPath, "utf8")); }
  catch (error) { if (error.code !== "ENOENT") throw error; }
  const entries = [];
  // Deliberately modest concurrency for CommunityDragon.
  for (let i = 0; i < assets.length; i += 4) {
    entries.push(...await Promise.all(assets.slice(i, i + 4).map(async ([file, path]) => {
      const url = base + path;
      const existing = previous?.assets.find(asset => asset.file === file);
      if (existing) {
        try {
          const bytes = await readFile(resolve(output, file));
          if (hash(bytes) === existing.sha256) return existing;
        } catch (error) { if (error.code !== "ENOENT") throw error; }
      }
      const response = await fetch(url);
      if (!response.ok) throw new Error(`${response.status}: ${url}`);
      const bytes = Buffer.from(await response.arrayBuffer());
      const sha256 = hash(bytes);
      const expected = previous?.assets.find(asset => asset.file === file)?.sha256;
      if (expected && sha256 !== expected) throw new Error(`Upstream changed: ${file}. Review before updating the pinned manifest.`);
      await mkdir(dirname(resolve(output, file)), { recursive: true });
      await writeFile(resolve(output, file), bytes);
      return { file, url, bytes: bytes.length, sha256 };
    })));
  }
  await writeFile(manifestPath, JSON.stringify({ patch: "16.20", copyright: "Riot Games, Inc. All rights reserved. See THIRD_PARTY_ASSETS.md.", assets: entries }, null, 2) + "\n");
  console.log(`Fetched ${entries.length} client assets (${(entries.reduce((total, item) => total + item.bytes, 0) / 1024 / 1024).toFixed(2)} MiB).`);
} else {
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  for (const asset of manifest.assets) {
    const bytes = await readFile(resolve(output, asset.file));
    if (hash(bytes) !== asset.sha256) throw new Error(`Asset hash mismatch: ${asset.file}`);
  }
  console.log(`Verified ${manifest.assets.length} bundled client assets.`);
}
