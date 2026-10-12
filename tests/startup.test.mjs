import test from "node:test";
import assert from "node:assert/strict";
import { demoBrowser } from "../scripts/demo-browser.mjs";

test("startup shows only the loading screen until the interface is ready", { timeout: 90000 }, async t => {
  const demo = await demoBrowser();
  try {
    await t.test("cold loads wait for app code, fonts, crests and CSS backgrounds", async () => {
      const page = await demo.browser.newPage();
      await page.emulateMediaFeatures([{ name: "prefers-reduced-motion", value: "no-preference" }]);
      await page.setCacheEnabled(false);
      await page.setRequestInterception(true);
      const blocked = new Set(["module", "font", "crest", "background"]);
      const pending = new Map([...blocked].map(type => [type, []]));
      page.on("request", request => {
        const path = new URL(request.url()).pathname;
        const type = path === "/src/main.ts" ? "module"
          : path.includes("/league/fonts/") ? "font"
          : path === "/league/ranks/challenger.png" ? "crest"
          : path === "/league/backgrounds/hextech.jpg" ? "background" : null;
        if (blocked.has(type)) pending.get(type).push(request);
        else void request.continue();
      });
      const release = async type => {
        blocked.delete(type);
        await Promise.all(pending.get(type).map(request => request.continue()));
      };
      const assertLoading = async () => {
        assert.deepEqual(await page.evaluate(() => ({
          ready: document.body.classList.contains("ready"),
          header: getComputedStyle(document.querySelector(".titlebar")).visibility,
          accounts: getComputedStyle(document.querySelector(".shell")).visibility,
          loader: getComputedStyle(document.querySelector("#startup-loader")).backgroundColor,
          busy: document.body.getAttribute("aria-busy"),
        })), { ready: false, header: "hidden", accounts: "hidden", loader: "rgb(6, 14, 23)", busy: "true" });
      };
      try {
        await page.goto(demo.base, { waitUntil: "domcontentloaded" });
        await assertLoading();
        assert.equal(await page.$eval(".brand-icon", el => el.getBoundingClientRect().width), 28);
        assert.equal(await page.$eval("#startup-loader", el => getComputedStyle(el, "::before").animationName), "startup-spin");
        await page.emulateMediaFeatures([{ name: "prefers-reduced-motion", value: "reduce" }]);
        assert.equal(await page.$eval("#startup-loader", el => getComputedStyle(el, "::before").animationName), "none");
        await page.emulateMediaFeatures([{ name: "prefers-reduced-motion", value: "no-preference" }]);
        assert.equal(await page.$eval("#startup-loader", el => getComputedStyle(el, "::before").animationName), "startup-spin");
        await release("module");
        await page.waitForSelector("#accounts > .card");
        await page.waitForFunction(() => document.fonts.status === "loading");
        await assertLoading();
        await release("font");
        await page.evaluate(() => document.fonts.ready.then(() => true));
        await assertLoading();
        await release("crest");
        await page.waitForFunction(() => [...document.images].every(img => img.complete && img.naturalWidth > 0));
        await assertLoading();
        await release("background");
        await page.waitForSelector("body.ready");
        assert.equal(await page.$("#startup-loader"), null);
        assert.equal(await page.$eval(".titlebar", el => getComputedStyle(el).visibility), "visible");
        assert.equal(await page.$eval(".brand-icon", el => el.getBoundingClientRect().width), 28);
        assert.equal(await page.$$eval(".card.enter, .card.enter-stagger", elements => elements.length), 0);
        assert.equal(await page.$eval("body", el => el.getAttribute("aria-busy")), "false");
      } finally {
        await page.close();
      }
    });
    await t.test("failed app modules show a retry action without revealing a broken interface", async () => {
      const page = await demo.browser.newPage();
      await page.setCacheEnabled(false);
      await page.setRequestInterception(true);
      let fail = true;
      page.on("request", request => {
        if (fail && new URL(request.url()).pathname === "/src/main.ts") {
          fail = false;
          void request.abort("failed");
        } else void request.continue();
      });
      try {
        await page.goto(demo.base, { waitUntil: "domcontentloaded" });
        await page.waitForSelector("#startup-error", { visible: true });
        assert.equal(await page.$eval(".titlebar", el => getComputedStyle(el).visibility), "hidden");
        assert.equal(await page.$("body.ready"), null);
        await Promise.all([page.waitForNavigation(), page.click("#startup-retry")]);
        await page.waitForSelector("body.ready");
        assert.equal(await page.$("#startup-loader"), null);
        assert.equal(await page.$$eval("#accounts > .card", cards => cards.length), 11);
      } finally {
        await page.close();
      }
    });
  } finally {
    await demo.close();
  }
});
