import test from "node:test";
import assert from "node:assert/strict";
import { demoBrowser, demoPage } from "../scripts/demo-browser.mjs";

test("account collection, client frames and silent motion", { timeout: 180000 }, async t => {
  const demo = await demoBrowser();
  const errors = [];
  let page;
  async function open(scene = "main", size) {
    await page?.close();
    page = await demoPage(demo.browser, demo.base, scene, size);
    page.on("pageerror", error => errors.push(error.message));
    page.on("response", response => { if (response.status() >= 400) errors.push(`${response.status()} ${response.url()}`); });
    return page;
  }
  const text = selector => page.$eval(selector, el => el.textContent);
  const count = () => page.$$eval("#accounts > .card", elements => elements.length);
  async function noHorizontalOverflow(selector) {
    assert.equal(await page.$eval(selector, el => el.scrollWidth <= el.clientWidth + 1), true, `${selector} overflows horizontally`);
  }
  try {
    await t.test("browser dev data covers every rank in both games and resets edits", async () => {
      await open();
      assert.equal(await page.$eval("#dev-tools", el => el.hidden), false);
      const expected = ["Challenger", "Grandmaster", "Master", "Diamond", "Emerald", "Platinum", "Gold", "Silver", "Bronze", "Iron", "Unranked"];
      for (const mode of ["lol", "tft"]) {
        await page.click(`.mode-btn[data-mode="${mode}"]`);
        assert.equal(await count(), 11);
        const distribution = await page.$$eval(".dist-row", rows => rows.map(row => [row.dataset.tier, row.querySelector(".dist-count").textContent]));
        assert.deepEqual(distribution, expected.map(tier => [tier, "1"]));
      }
      await page.click('#accounts > .card:first-child [data-act="menu"]');
      await page.click('.menu-item[data-index="1"]');
      await page.waitForSelector('.modal [name="name"]');
      await page.$eval('.modal [name="name"]', el => { el.value = "Edited#DEV"; });
      await page.click('.modal [data-value="save"]');
      await page.waitForSelector('.modal', { hidden: true });
      assert.match(await text("#accounts"), /Edited/);
      await Promise.all([
        page.waitForNavigation({ waitUntil: "networkidle0" }),
        page.click("#reset-demo"),
      ]);
      await page.waitForSelector("body.ready #accounts > .card");
      assert.equal(await count(), 11);
      assert.doesNotMatch(await text("#accounts"), /Edited/);
      assert.equal(await page.$eval("html", el => el.dataset.mode), "lol");
    });
    await t.test("rank, region and search filters keep counts and reset in sync", async () => {
      await open();
      assert.equal(await count(), 11);
      assert.equal(await text("#account-count"), "11 of 11 accounts");
      await page.select("#region-filter", "EUW");
      assert.equal(await count(), 3);
      await page.type("#search", "Hextech");
      assert.equal(await count(), 1);
      assert.equal(await text("#account-count"), "1 of 11 accounts");
      await page.click("#clear-filters");
      assert.equal(await count(), 11);
      await page.click('.dist-row[data-tier="Diamond"]');
      assert.equal(await count(), 1);
      await page.click("#clear-filters");
      await page.select("#sort", "level");
      assert.match(await text("#accounts > .card:first-child"), /Lux Main/);
      await page.click("#accounts > .card:first-child .identity");
      await page.keyboard.press("ArrowRight");
      assert.match(await text("#accounts > .selected"), /Hextech Fox/);
    });
    await t.test("game modes use their respective client crests; compact view fits minimum size", async () => {
      await open();
      await page.click('.mode-btn[data-mode="tft"]');
      assert.match(await text("#queue-label"), /Teamfight Tactics/i);
      await page.waitForFunction(() => [...document.querySelectorAll("#accounts .emblem img")].every(img => img.complete && img.naturalWidth > 0));
      assert.equal(await page.$$eval("#accounts .emblem img", imgs => imgs.every(img => (img.src.endsWith("-tft.svg") || img.src.endsWith("/ranks/unranked.png")) && img.naturalWidth > 0)), true);
      await page.click('[data-view="list"]');
      await page.setViewport({ width: 980, height: 640 });
      await noHorizontalOverflow(".main");
      await noHorizontalOverflow("#accounts");
      await page.click('.mode-btn[data-mode="lol"]');
      assert.match(await text("#queue-label"), /Solo/);
      assert.equal(await count(), 11);
    });
    await t.test("selected game tabs visibly respond to keyboard focus with either motion preference", async () => {
      await open();
      const appearance = selector => page.$eval(selector, el => ({
        focused: el.matches(":focus-visible"),
        selected: el.getAttribute("aria-selected"),
        outline: getComputedStyle(el).outlineStyle,
        paint: {
          background: getComputedStyle(el).backgroundImage,
          navigationLight: getComputedStyle(el, "::after").opacity,
        },
      }));
      for (const motion of ["no-preference", "reduce"]) {
        await page.emulateMediaFeatures([{ name: "prefers-reduced-motion", value: motion }]);
        for (const mode of ["lol", "tft"]) {
          const selector = `.mode-btn[data-mode="${mode}"]`;
          await page.click(selector);
          await page.mouse.move(0, 90);
          await page.keyboard.press("Tab");
          const resting = await appearance(selector);
          assert.equal(resting.focused, false);
          await page.keyboard.down("Shift");
          await page.keyboard.press("Tab");
          await page.keyboard.up("Shift");
          const focused = await appearance(selector);
          assert.equal(focused.focused, true);
          assert.equal(focused.selected, "true", "focusing a tab must preserve selection");
          assert.equal(focused.outline, "none", "use the client's gold navigation light, not a generic outline");
          assert.notDeepEqual(focused.paint, resting.paint, `${mode} focus must stay visible with ${motion}`);
        }
      }
    });
    await t.test("Settings frame contains scrolling content while its actions and close stay reachable", async () => {
      await open("settings", { width: 980, height: 640 });
      await page.waitForSelector(".modal");
      assert.equal(await page.$eval(".modal-body", el => el.scrollHeight > el.clientHeight), true);
      const bounds = await page.$eval(".modal", el => {
        const r = el.getBoundingClientRect();
        const c = el.querySelector(".modal-close").getBoundingClientRect();
        const f = el.querySelector(".modal-foot").getBoundingClientRect();
        return r.left >= 0 && r.right <= innerWidth && c.top >= 64 && c.right <= innerWidth && f.bottom <= innerHeight;
      });
      assert.equal(bounds, true);
      await noHorizontalOverflow(".modal-body");
      await page.focus('.modal [name="launchAtStartup"]');
      await page.keyboard.press("Space");
      assert.equal(await page.$eval('[name="launchAtStartup"]', el => el.checked), true);
      await page.focus('.modal [name="loginMethod"][value="previous"]');
      await page.keyboard.press("Space");
      assert.equal(await page.$eval('[name="launchGame"]', el => el.disabled), true);
      await page.focus('.modal [data-value="save"]');
      await page.keyboard.press("Tab");
      assert.equal(await page.$eval(".modal-close", el => el === document.activeElement), true);
      await page.keyboard.down("Shift");
      await page.keyboard.press("Tab");
      await page.keyboard.up("Shift");
      assert.equal(await page.$eval('[data-value="save"]', el => el === document.activeElement), true);
      await page.select('.modal [name="language"]', "fr");
      await page.click('.modal [data-value="save"]');
      await page.waitForSelector(".modal", { hidden: true });
      assert.equal(await text("#account-count"), "11 comptes sur 11");
    });
    await t.test("client tooltips support focus, escaping, dismissal and removed owners", async () => {
      await open();
      const selector = '#accounts > .card:first-child [data-act="copy-pass"]';
      await page.focus(selector);
      await page.waitForSelector('[role="tooltip"]');
      assert.equal(await text('[role="tooltip"]'), "Copy password");
      assert.equal(await page.$eval(selector, el => el.getAttribute("aria-describedby")), "client-tooltip");
      await page.keyboard.press("Escape");
      assert.equal(await page.$('[role="tooltip"]'), null);
      assert.equal(await page.$eval(selector, el => el.hasAttribute("aria-describedby")), false);
      await page.$eval(selector, el => { el.dataset.tooltip = '<img src=x onerror="alert(1)">'; });
      await page.hover(selector);
      await page.waitForSelector('[role="tooltip"]');
      assert.equal(await page.$eval('[role="tooltip"]', el => el.children.length), 0);
      const bounds = await page.$eval('[role="tooltip"]', el => { const r = el.getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth && r.top >= 0 && r.bottom <= innerHeight; });
      assert.equal(bounds, true);
      await page.select("#region-filter", "EUW");
      await page.waitForSelector('[role="tooltip"]', { hidden: true });
    });
    await t.test("adding an account without a Riot ID retains one-click login and the edit action", async () => {
      await open("add");
      await page.waitForSelector(".drawer");
      await page.type('.drawer [name="accountId"]', "ui_test_only");
      await page.type('.drawer [name="password"]', "synthetic-test-password");
      await page.click('.drawer [data-panel="single"] button[type="submit"]');
      await page.waitForFunction(() => document.querySelectorAll("#accounts > .card").length === 12);
      await page.click(".drawer [data-close]");
      await page.waitForSelector(".drawer", { hidden: true });
      await page.type("#search", "ui_test_only");
      assert.equal(await count(), 1);
      assert.match(await text("#accounts"), /No Riot ID associated/);
      assert.ok(await page.$("#accounts .rank-edit"));
      assert.equal(await page.$eval("#accounts .btn-play", el => el.disabled), false);
    });
    await t.test("rank crests stay static on hover and focus; login respects reduced motion", async () => {
      await open();
      await page.hover("#accounts > .card:nth-child(3)");
      await page.focus("#accounts > .card:nth-child(3)");
      assert.equal(await page.$$eval("video,audio,[data-crest-motion]", elements => elements.length), 0);
      await page.emulateMediaFeatures([{ name: "prefers-reduced-motion", value: "reduce" }]);
      await page.hover("#accounts > .card:first-child");
      assert.equal(await page.$$eval("video,audio", els => els.length), 0);
      await page.click("#accounts > .card:first-child .btn-play");
      await page.waitForSelector(".login-overlay");
      assert.equal(await page.$$eval("video,audio", els => els.length), 0);
      assert.equal(await page.$(".is-client-click"), null);
    });
    await t.test("login progress follows completed steps without video or audio", async () => {
      await open("login");
      await page.waitForSelector(".login-overlay");
      assert.equal(await page.$$eval("video,audio", els => els.length), 0);
      await page.waitForFunction(() => Number(document.querySelector('[role="progressbar"]').getAttribute("aria-valuenow")) > 0);
      const progress = await page.$eval('[role="progressbar"]', el => ({
        value: Number(el.getAttribute("aria-valuenow")),
        max: Number(el.getAttribute("aria-valuemax")),
        resolved: document.querySelectorAll(".login-steps .done, .login-steps .skipped").length,
        current: el.getAttribute("aria-valuetext"),
        label: document.querySelector(".login-steps .active .step-label").textContent,
      }));
      assert.equal(progress.value, progress.resolved);
      assert.ok(progress.value < progress.max);
      assert.equal(progress.current, progress.label);
      await page.waitForSelector(".login-overlay.is-success", { timeout: 15000 });
      assert.equal(await page.$$eval("video,audio", els => els.length), 0);
      assert.equal(await page.$eval('[role="progressbar"]', el => el.getAttribute("aria-valuenow") === el.getAttribute("aria-valuemax")), true);
      await page.click('[data-login="close"]');
      await page.waitForSelector(".login-overlay", { hidden: true });
    });
    assert.deepEqual(errors, []);
  } finally {
    await demo.close();
  }
});
