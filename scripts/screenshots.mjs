// Capture README screenshots and GIFs from the dev server's demo data.
//
// Usage: `npm run dev` in another terminal, then `node scripts/screenshots.mjs`.
// Requires Chrome and ffmpeg on PATH (GIFs are converted from screencasts).
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import puppeteer from "puppeteer-core";

const CHROME = process.env.CHROME_PATH ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const BASE = process.env.BASE_URL ?? "http://localhost:1420/";
const OUT = "docs/screenshots";
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Static screens: scene -> wait for animations to settle.
const STILLS = { main: 2600, list: 2600, settings: 1500, add: 1500, menu: 1500 };

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

function toGif(input, output) {
  const filter =
    "fps=12,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=160:stats_mode=diff[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle";
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-i", input, "-vf", filter, "-loop", "0", output]);
}

mkdirSync(OUT, { recursive: true });
mkdirSync(`${OUT}/.tmp`, { recursive: true });
const browser = await puppeteer.launch({ executablePath: CHROME, args: ["--lang=en-US"] });

async function open(scene) {
  const page = await browser.newPage();
  await page.setViewport({ width: 1360, height: 860, deviceScaleFactor: 1 });
  await page.evaluateOnNewDocument(() => localStorage.setItem("leagueaccounts.lang", "en"));
  await page.goto(`${BASE}?scene=${scene}`, { waitUntil: "domcontentloaded" });
  return page;
}

try {
  for (const [scene, wait] of Object.entries(STILLS)) {
    const page = await open(scene);
    await sleep(wait);
    await page.screenshot({ path: `${OUT}/${scene}.png` });
    await page.close();
    console.log(`still ${scene}`);
  }
  for (const [scene, act] of Object.entries(CLIPS)) {
    const page = await browser.newPage();
    await page.setViewport({ width: 1360, height: 860, deviceScaleFactor: 1 });
    await page.evaluateOnNewDocument(() => localStorage.setItem("leagueaccounts.lang", "en"));
    const recorder = await page.screencast({ path: `${OUT}/.tmp/${scene}.webm` });
    await page.goto(`${BASE}?scene=${scene === "overview" ? "main" : scene}`, { waitUntil: "domcontentloaded" });
    await act(page);
    await recorder.stop();
    await page.close();
    toGif(`${OUT}/.tmp/${scene}.webm`, `${OUT}/${scene}.gif`);
    console.log(`gif ${scene}`);
  }
} finally {
  await browser.close();
  rmSync(`${OUT}/.tmp`, { recursive: true, force: true });
}
