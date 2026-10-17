// Shared by visual captures and browser regressions. Uses only synthetic data.
import { createServer } from "vite";
import puppeteer from "puppeteer-core";
import { fileURLToPath } from "node:url";

export async function demoBrowser() {
  const server = process.env.BASE_URL ? null : await createServer({
    configFile: fileURLToPath(new URL("../vite.config.ts", import.meta.url)),
    server: { host: "127.0.0.1", port: 0, strictPort: false },
  });
  await server?.listen();
  const base = process.env.BASE_URL ?? server.resolvedUrls.local[0];
  let browser;
  try {
    browser = await puppeteer.launch({
      executablePath: process.env.CHROME_PATH ?? "C:/Program Files/Google/Chrome/Application/chrome.exe",
      args: ["--lang=en-US"],
    });
  } catch (error) {
    await server?.close();
    throw error;
  }
  return {
    browser,
    base,
    async close() {
      // On Windows, a headless Chrome used for screencasts can linger after
      // Browser.close. Bound cleanup to this helper's own temporary process.
      let timeout;
      try {
        await Promise.race([
          browser.close(),
          new Promise(resolve => {
            timeout = setTimeout(() => { browser.process()?.kill(); resolve(); }, 10000);
          }),
        ]);
      } finally {
        clearTimeout(timeout);
        await browser.disconnect();
        await server?.close();
      }
    },
  };
}

export async function demoPage(browser, base, scene = "main", viewport = { width: 1360, height: 860 }) {
  const page = await browser.newPage();
  await page.setViewport({ ...viewport, deviceScaleFactor: 1 });
  await page.evaluateOnNewDocument(() => {
    localStorage.clear();
    localStorage.setItem("leagueaccounts.dev.lang", "en");
  });
  await page.goto(`${base}?scene=${scene}`, { waitUntil: "networkidle0" });
  await page.waitForSelector("body.ready #accounts > .card");
  await page.evaluate(async () => {
    await document.fonts.ready;
    await Promise.all([...document.images].map(image => image.decode().catch(() => {})));
  });
  return page;
}
