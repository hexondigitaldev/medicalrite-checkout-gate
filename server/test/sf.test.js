// Theme script (extensions/storefront-helper/assets/sf.js) in a fake browser.
import test, { after } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM, VirtualConsole } from "jsdom";

const SRC = readFileSync(new URL("../../extensions/storefront-helper/assets/sf.js", import.meta.url), "utf8");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const windows = [];
after(() => windows.forEach((w) => w.close()));

function browser({ items = [{ variant_id: 11, quantity: 1 }], attr = null, cartOk = true, epDelay = 0, stored = null, epStatus = 200, formExtra = "" } = {}) {
  const dom = new JSDOM(`<!doctype html><html><head></head><body>
    <form id="cart" action="/cart" method="post">${formExtra}<button id="co" type="submit" name="checkout">Checkout</button></form><div class="additional-checkout-buttons" id="ex"></div>
  </body></html>`, { url: "https://shop.example/cart", runScripts: "dangerously", virtualConsole: new VirtualConsole() });
  const w = dom.window;
  windows.push(w);
  const state = { items, attr, calls: [], submits: 0 };
  const res = (body, ok = true) => ({ ok, json: async () => body });
  w.fetch = async (url, init = {}) => {
    const u = String(url);
    const isForm = init.body && typeof init.body !== "string";
    state.calls.push({ u, body: init.body && !isForm ? JSON.parse(init.body) : null, attrAtCall: state.attr });
    if (u.endsWith("cart/update.js") && isForm) {
      const q = Number(init.body.get("updates[]"));
      state.items = [{ variant_id: 11, quantity: q }];
      return res({});
    }
    if (u.endsWith("cart.js")) return cartOk ? res({ items: state.items, attributes: state.attr ? { _bg: state.attr } : {} }) : res({}, false);
    if (u === "/apps/sc/t") {
      await sleep(epDelay);
      if (epStatus !== 200) return res({ error: "slow_down" }, false);
      const c = JSON.parse(init.body).c;
      return res({ t: `tok-for-${c}`, f: JSON.parse(init.body).r ? "h" : "s", ttl: 3000 });
    }
    if (u.endsWith("cart/update.js")) { state.attr = JSON.parse(init.body).attributes._bg; return res({}); }
    if (u.includes("/cart/change")) { state.items = JSON.parse(init.body).items; return res({}); }
    return res({}, false);
  };
  // Turnstile stub: answers after 10 ms.
  new w.MutationObserver(() => {
    for (const s of w.document.querySelectorAll("script[src*='challenges']")) {
      if (s.dataset.done) continue;
      s.dataset.done = 1;
      w.turnstile = { render: (_el, o) => ((w.__o = o), 1), reset() {}, execute: () => setTimeout(() => w.__o.callback("ts-ok"), 10) };
      setTimeout(() => w.__sfhTs(), 1);
    }
  }).observe(w.document.head, { childList: true });
  if (stored) w.sessionStorage.setItem("sfh", JSON.stringify(stored));
  const s = w.document.createElement("script");
  s.setAttribute("data-k", "sitekey");
  s.textContent = SRC;
  w.document.body.appendChild(s);
  // Counts submits that survive every handler (registered after sf.js, so it runs last).
  w.addEventListener("submit", (e) => { if (!e.defaultPrevented) { state.submits++; state.attrAtSubmit = state.attr; } e.preventDefault(); });
  return { w, state };
}
const count = (state, part) => state.calls.filter((c) => c.u.includes(part)).length;

test("page load with items: gets a token and saves it on the cart", async () => {
  const { state } = browser();
  await sleep(150);
  assert.equal(state.attr, "tok-for-11:1");
  assert.equal(count(state, "/apps/sc/t"), 1);
});

test("fresh saved token for the same cart: no new request", async () => {
  const { state } = browser({ attr: "T", stored: { t: "T", c: "11:1", f: "h", at: Date.now(), exp: Date.now() + 50 * 60000 } });
  await sleep(150);
  assert.equal(count(state, "/apps/sc/t"), 0);
});

test("token close to expiry is refreshed on load", async () => {
  const { state } = browser({ attr: "T", stored: { t: "T", c: "11:1", f: "h", at: Date.now() - 40 * 60000, exp: Date.now() + 10 * 60000 } });
  await sleep(150);
  assert.equal(count(state, "/apps/sc/t"), 1);
});

test("empty cart: nothing to do", async () => {
  const { state } = browser({ items: [] });
  await sleep(100);
  assert.equal(count(state, "/apps/sc/t"), 0);
});

test("race: cart changes while a refresh runs, checkout waits for the token of the NEW cart", async () => {
  const { w, state } = browser({ epDelay: 80 });
  await sleep(30); // first refresh in flight
  await w.fetch("/cart/change.js", { method: "POST", body: JSON.stringify({ items: [{ variant_id: 11, quantity: 2 }] }) });
  w.document.getElementById("co").click();
  assert.equal(state.submits, 0, "held");
  await sleep(1200);
  assert.equal(state.submits, 1, "released once");
  assert.equal(state.attrAtSubmit, "tok-for-11:2", "token matches the cart at the moment checkout starts");
});

test("cart.js failing: bounded retries, never a busy loop", async () => {
  const { state } = browser({ cartOk: false });
  await sleep(7000);
  const n = count(state, "cart.js");
  assert.ok(n >= 2 && n <= 4, `cart.js called ${n} times`);
});

test("soft token is reused for 5 minutes instead of retrying on every page", async () => {
  const { state } = browser({ attr: "S", stored: { t: "S", c: "11:1", f: "s", at: Date.now() - 60000, exp: Date.now() + 50 * 60000 } });
  await sleep(150);
  assert.equal(count(state, "/apps/sc/t"), 0);
});

test("classic cart form: typed quantity is applied, token refreshed for it, then checkout", async () => {
  const { w, state } = browser({ formExtra: '<input name="updates[]" value="1">' });
  await sleep(150);
  w.document.querySelector('[name="updates[]"]').value = "3";
  w.document.getElementById("co").click();
  await sleep(600);
  assert.equal(state.submits, 0, "native submit (which would skip the refresh) was stopped");
  assert.equal(state.attr, "tok-for-11:3", "token is for the quantity the buyer typed");
});

test("express buttons are disabled only while a refresh after a cart change is pending", async () => {
  const { w } = browser({ epDelay: 100 });
  await sleep(250);
  const html = w.document.documentElement;
  assert.equal(html.classList.contains("sfh-busy"), false);
  const p = w.fetch("/cart/add.js", { method: "POST", body: JSON.stringify({}) });
  assert.equal(html.classList.contains("sfh-busy"), true);
  await p;
  await sleep(700);
  assert.equal(html.classList.contains("sfh-busy"), false);
});

test("near-expiry token: checkout never starts with the old token", async () => {
  const { w, state } = browser({ attr: "T", stored: { t: "T", c: "11:1", f: "h", at: Date.now() - 6 * 60000, exp: Date.now() + 20 * 60000 } });
  w.document.getElementById("co").click(); // clicked immediately, while the load check runs
  await sleep(400);
  assert.equal(state.submits, 1);
  assert.equal(state.attrAtSubmit, "tok-for-11:1");
});

test("429 from the endpoint: bounded retries, page keeps working", async () => {
  const { w, state } = browser({ epStatus: 429 });
  await sleep(7000);
  const n = count(state, "/apps/sc/t");
  assert.ok(n >= 2 && n <= 4, `endpoint called ${n} times`);
  w.document.getElementById("co").click();
  await sleep(50);
  assert.equal(state.submits, 1, "checkout not trapped");
});

test("a theme handler that cancels checkout (e.g. terms not ticked) wins", async () => {
  const { w, state } = browser({ formExtra: '<input name="updates[]" value="1">' });
  await sleep(150);
  w.document.getElementById("cart").addEventListener("submit", (e) => e.preventDefault());
  const before = state.calls.length;
  w.document.getElementById("co").click();
  await sleep(300);
  assert.equal(state.calls.slice(before).filter((c) => c.u.includes("cart/update.js")).length, 0, "no update, no navigation");
});

test("theme 'disable on submit' handler + pending refresh: resumed submit still carries checkout", async () => {
  const { w, state } = browser({ epDelay: 80 });
  let runs = 0, lastSubmitterDisabled = null;
  w.document.getElementById("cart").addEventListener("submit", (e) => {
    runs++;
    lastSubmitterDisabled = e.submitter ? e.submitter.disabled : null;
    if (e.submitter) e.submitter.disabled = true;
  });
  await sleep(20); // refresh in flight
  w.document.getElementById("cart").requestSubmit(w.document.getElementById("co"));
  await sleep(600);
  assert.equal(runs, 1, "theme handler ran exactly once");
  assert.equal(lastSubmitterDisabled, false, "submitter was still enabled when the real submit happened");
});
