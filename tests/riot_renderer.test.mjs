import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { constants, runInNewContext } from "node:vm";

const source = await readFile(new URL("../src/riot_renderer.js", import.meta.url), "utf8");
let sequence = 0;

async function form(t, response) {
  const fields = Object.fromEntries(["username", "password"].map(name => {
    const input = { value: "", disabled: false };
    input.__reactProps$test = {
      value: "",
      onChange: () => { input.__reactProps$test.value = input.value; },
    };
    return [name, input];
  }));
  const binding = { post: async () => response };
  // The production dynamic import resolves a module exporting getBinding.
  // Give each fixture its own module and remove its dummy binding afterward.
  const key = `leagueaccounts-renderer-test-${++sequence}`;
  globalThis[key] = binding;
  t.after(() => { delete globalThis[key]; });
  const script = `data:text/javascript,export const bind = () => globalThis[${JSON.stringify(key)}]; // /renderer.js`;
  let error = false;
  let captcha = false;
  const context = {
    navigator: { webdriver: false },
    window: { riotInvoke() {} },
    setTimeout,
    URL,
    fetch: async () => ({ text: async () => "export { getBinding as bind }" }),
    getComputedStyle: () => ({ visibility: "visible", opacity: "1" }),
    document: {
      scripts: [{ src: script }],
      querySelector(selector) {
        const name = selector.match(/^input\[name="(.*)"\]$/)?.[1];
        if (name) return fields[name];
        if (selector === '[data-testid="login-form"]') return { dataset: { loggingIn: "false" } };
        if (selector === '[data-testid="login-form-error-tooltip"]') return error ? {} : null;
        if (selector === '[aria-label="hcaptcha challenge"]') {
          return captcha ? { getBoundingClientRect: () => ({ width: 100 }) } : null;
        }
        return null;
      },
    },
  };
  runInNewContext(source, context, {
    importModuleDynamically: constants.USE_MAIN_CONTEXT_DEFAULT_LOADER,
  });
  const login = context.__leagueAccountsLogin;
  assert.equal(await login.fill("dummy-user", "dummy-password"), true);
  await binding.post("/rso-authenticator/v1/authentication/riot-identity/complete", {
    username: "dummy-user", password: "dummy-password",
  });
  return {
    status: () => login.status(),
    reject() { fields.password.value = ""; error = true; },
    showCaptcha() { captcha = true; },
  };
}

test("MFA followed by a reset login form reports rejection immediately", async t => {
  const login = await form(t, { type: "multifactor" });
  assert.equal(login.status(), "verification");
  login.reject();
  assert.equal(login.status(), "rejected");
});

test("terminal authentication errors are not hidden by a visible challenge", async t => {
  for (const [error, expected] of [
    ["captcha_invalid", "captcha_rejected"],
    ["rate_limited", "rate_limited"],
    ["auth_failure", "rejected"],
  ]) {
    const login = await form(t, { error });
    login.showCaptcha();
    login.reject();
    assert.equal(login.status(), expected);
  }
});

test("pending CAPTCHA and successful authentication retain their states", async t => {
  const pending = await form(t, {});
  assert.equal(pending.status(), "pending");
  pending.showCaptcha();
  assert.equal(pending.status(), "verification");
  const success = await form(t, { type: "success" });
  assert.equal(success.status(), "success");
});
