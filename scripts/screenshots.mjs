// Capture README screenshots and GIFs from the real UI with synthetic data.
//
// Usage: `npm run screenshots`, or `npm run screenshots -- login` for one scene.
// Starts an isolated dev server automatically.
// Requires Chrome and ffmpeg on PATH (GIFs are converted from screencasts).
import { execFileSync } from "node:child_process";
import { mkdirSync, renameSync, rmSync } from "node:fs";
import { resolve } from "node:path";
import { demoBrowser, demoPage } from "./demo-browser.mjs";

const OUT = resolve("docs/screenshots");
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Static screens: scene -> wait for animations to settle.
const STILLS = { main: 1000, tft: 1000, list: 1000, settings: 1000, add: 1000, menu: 1000, login: 2200 };

// Animated screens: scene -> scripted interaction while recording.
const CLIPS = {
  overview: async (page) => {
    await sleep(2600);
    const card = await page.$("#accounts > .card:nth-child(3)");
    const box = await card.boundingBox();
    await page.mouse.move(box.x + 60, box.y + 40, { steps: 12 });
    await sleep(900);
    await page.mouse.move(box.x + box.width - 40, box.y + 120, { steps: 16 });
    await sleep(700);
    await page.click('.mode-btn[data-mode="tft"]');
    await sleep(2400);
    await page.click('.mode-btn[data-mode="lol"]');
    await sleep(2200);
  },
  login: async () => {
    await sleep(9000);
  },
};

const requested = new Set(process.argv.slice(2));
for (const scene of requested) {
  if (!(scene in STILLS) && !(scene in CLIPS)) throw new Error(`Unknown capture scene: ${scene}`);
}

function toGif(input, output) {
  const filter =
    "fps=12,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=160:stats_mode=diff[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle";
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-i", input, "-vf", filter, "-loop", "0", output]);
}

mkdirSync(OUT, { recursive: true });
mkdirSync(`${OUT}/.tmp`, { recursive: true });
const demo = await demoBrowser();
const { browser, base } = demo;

try {
  for (const [scene, wait] of Object.entries(STILLS)) {
    if (requested.size && !requested.has(scene)) continue;
    const page = await demoPage(browser, base, scene);
    await sleep(wait);
    await page.screenshot({ path: `${OUT}/.tmp/${scene}.png` });
    renameSync(`${OUT}/.tmp/${scene}.png`, `${OUT}/${scene}.png`);
    await page.close();
    console.log(`still ${scene}`);
  }
  for (const [scene, act] of Object.entries(CLIPS)) {
    if (requested.size && !requested.has(scene)) continue;
    const page = await demoPage(browser, base);
    const recorder = await page.screencast({ path: `${OUT}/.tmp/${scene}.webm` });
    try {
      if (scene === "login") await page.click('#accounts > .card:nth-child(3) .btn-play');
      await act(page);
    } finally {
      await recorder.stop();
    }
    // Let in-flight CDP frame acknowledgements settle before closing the target.
    await sleep(250);
    await page.close();
    toGif(`${OUT}/.tmp/${scene}.webm`, `${OUT}/.tmp/${scene}.gif`);
    renameSync(`${OUT}/.tmp/${scene}.gif`, `${OUT}/${scene}.gif`);
    console.log(`gif ${scene}`);
  }
} finally {
  await demo.close();
  // This is the fixed, resolved scratch directory created above.
  const scratch = resolve(OUT, ".tmp");
  if (!scratch.startsWith(`${OUT}\\`) && !scratch.startsWith(`${OUT}/`)) throw new Error("Invalid capture scratch path");
  rmSync(scratch, { recursive: true, force: true });
}
