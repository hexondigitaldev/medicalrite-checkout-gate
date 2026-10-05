(function () {
  "use strict";
  var me = document.currentScript;
  var KEY = me && me.getAttribute("data-k");
  if (!KEY || window.__sfh) return;
  window.__sfh = true;

  var root = (window.Shopify && Shopify.routes && Shopify.routes.root) || "/";
  var EP = "/apps/sc/t";
  var ATTR = "_bg";
  var MIN_LEFT_MS = 25 * 60 * 1000;
  var RETRY_S_MS = 5 * 60 * 1000;
  var TS_WAIT_MS = 4000;
  var EP_WAIT_MS = 2500;
  var HOLD_MS = 12000;
  var BACKOFF = [1000, 5000, 30000];
  var STORE = "sfh";
  var CART_WRITE = /\/cart\/(add|change|update|clear)(\.js)?(\?|$)/;
  var EXPRESS = "shopify-accelerated-checkout-cart, shopify-accelerated-checkout, .additional-checkout-buttons, .dynamic-checkout__content";

  var origFetch = window.fetch.bind(window);
  var running = null, dirty = true, passthrough = false, timer = null, fails = 0;

  function now() { return Date.now(); }
  function sleep(ms) { return new Promise(function (r) { setTimeout(r, ms); }); }
  function withTimeout(p, ms) {
    return Promise.race([p, new Promise(function (_, rej) { setTimeout(function () { rej(new Error("t")); }, ms); })]);
  }
  function saved() { try { return JSON.parse(sessionStorage.getItem(STORE) || "null"); } catch (e) { return null; } }
  function save(v) { try { sessionStorage.setItem(STORE, JSON.stringify(v)); } catch (e) {} }
  var busyTimer = null;
  function busy(on) {
    document.documentElement.classList.toggle("sfh-busy", !!on);
    clearTimeout(busyTimer);
    if (on) busyTimer = setTimeout(function () { busy(false); }, HOLD_MS);
  }

  var css = document.createElement("style");
  css.textContent = ".sfh-busy :is(" + EXPRESS + "){pointer-events:none;opacity:.6;transition:opacity .2s}";
  document.head.appendChild(css);

  function cartContent(items) {
    var agg = {};
    (items || []).forEach(function (it) {
      var id = String(it.variant_id || "");
      var q = Number(it.quantity) || 0;
      if (!/^\d{1,19}$/.test(id)) return;
      agg[id] = (agg[id] || 0) + q;
    });
    return Object.keys(agg)
      .filter(function (id) { return agg[id] > 0; })
      .sort(function (a, b) { return a.length - b.length || (a < b ? -1 : a > b ? 1 : 0); })
      .map(function (id) { return id + ":" + agg[id]; })
      .join(",");
  }

  var tsLoad = null, widget = null, tsDone = null;
  function loadTs() {
    if (tsLoad) return tsLoad;
    tsLoad = new Promise(function (resolve, reject) {
      window.__sfhTs = function () { resolve(); };
      var s = document.createElement("script");
      s.src = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit&onload=__sfhTs";
      s.async = true;
      s.onerror = function () { tsLoad = null; reject(new Error("l")); };
      document.head.appendChild(s);
    });
    return tsLoad;
  }
  function finish(v) { if (tsDone) { var d = tsDone; tsDone = null; d(v); } }
  function tsResponse() {
    return withTimeout(loadTs().then(function () {
      return new Promise(function (resolve) {
        tsDone = resolve;
        if (widget === null) {
          var el = document.createElement("div");
          document.body.appendChild(el);
          widget = window.turnstile.render(el, {
            sitekey: KEY, action: "cart", execution: "execute", retry: "never",
            callback: function (t) { finish(t); },
            "error-callback": function () { finish(null); return true; },
            "timeout-callback": function () { finish(null); }
          });
        } else {
          window.turnstile.reset(widget);
        }
        window.turnstile.execute(widget);
      });
    }), TS_WAIT_MS).catch(function () { finish(null); return null; });
  }

  function fresh(s, c, cur) {
    if (!s || !cur || s.t !== cur || s.c !== c) return false;
    if (now() - s.at < RETRY_S_MS) return true;
    return s.f === "h" && s.exp - now() >= MIN_LEFT_MS;
  }

  function post(url, body, ms) {
    var ctl = window.AbortController ? new AbortController() : null;
    var t = ctl && setTimeout(function () { ctl.abort(); }, ms);
    return origFetch(url, {
      method: "POST", credentials: "same-origin",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body), signal: ctl ? ctl.signal : undefined
    }).finally(function () { clearTimeout(t); });
  }

  function run(force) {
    return (async function () {
      var r = await origFetch(root + "cart.js", { credentials: "same-origin", cache: "no-store" });
      if (!r.ok) throw new Error("c");
      var cart = await r.json();
      dirty = false;
      var c = cartContent(cart.items);
      if (!c) return;
      var cur = cart.attributes && cart.attributes[ATTR];
      if (!force && fresh(saved(), c, cur)) return;
      var resp = await post(EP, { c: c, r: await tsResponse() }, EP_WAIT_MS);
      if (!resp.ok) throw new Error("e");
      var data = await resp.json();
      if (!data || !data.t) throw new Error("d");
      var attrs = {};
      attrs[ATTR] = data.t;
      var up = await post(root + "cart/update.js", { attributes: attrs }, EP_WAIT_MS);
      if (!up.ok) throw new Error("u");
      save({ t: data.t, c: c, f: data.f, at: now(), exp: now() + (Number(data.ttl) || 1800) * 1000 });
    })();
  }

  function ensure(force) {
    if (running) return running;
    running = run(force).then(function () { fails = 0; }, function () {
      dirty = false;
      if (fails < BACKOFF.length) { var d = BACKOFF[fails++]; setTimeout(function () { ensure(force); }, d); }
    }).finally(function () {
      running = null;
      if (dirty) ensure(false);
      else busy(false);
    });
    return running;
  }

  function changed() {
    dirty = true;
    fails = 0;
    busy(true);
    clearTimeout(timer);
    timer = setTimeout(function () { ensure(false); }, 250);
  }

  window.fetch = function (input, init) {
    var url = String((input && input.url) || input || "");
    var p = origFetch(input, init);
    if (CART_WRITE.test(url)) { dirty = true; busy(true); p.then(changed, changed); }
    return p;
  };
  var xOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url) {
    if (CART_WRITE.test(String(url))) { dirty = true; busy(true); this.addEventListener("loadend", changed); }
    return xOpen.apply(this, arguments);
  };

  function nearExpiry(s) { return s && s.exp - now() < MIN_LEFT_MS && now() - s.at >= RETRY_S_MS; }
  function needsWork() { return running || dirty || nearExpiry(saved()); }
  async function settle() {
    var end = now() + HOLD_MS;
    if (!running && !dirty && nearExpiry(saved())) ensure(true);
    while ((running || dirty) && now() < end) {
      if (!running) { clearTimeout(timer); ensure(false); }
      await Promise.race([running || sleep(50), sleep(end - now())]);
    }
  }
  function hold(ev, el, resume) {
    if (passthrough || !needsWork()) return;
    ev.preventDefault();
    ev.stopImmediatePropagation();
    settle().then(function () {
      passthrough = true;
      try {
        if (el && !el.isConnected) location.href = root + "checkout";
        else resume();
      } catch (e) {
        location.href = root + "checkout";
      } finally {
        passthrough = false;
      }
    });
  }
  document.addEventListener("click", function (ev) {
    var el = ev.target && ev.target.closest &&
      ev.target.closest('button[name="checkout"], input[name="checkout"], a[href$="/checkout"], a[href*="/checkout?"]');
    if (!el) return;
    if (el.form && el.form.querySelector('[name^="updates"]')) return; // handled on submit below
    hold(ev, el, function () { el.click(); });
  }, true);
  function checkoutForm(ev) {
    var f = ev.target, sub = ev.submitter;
    if (!f || !f.getAttribute) return null;
    var isCheckout = (sub && sub.name === "checkout") || /\/checkout(\?|$)/.test(f.getAttribute("action") || "");
    return isCheckout ? f : null;
  }
  // Forms without quantity fields: hold in the capture phase and resume with requestSubmit,
  // so the theme's own submit handlers run exactly once, on the resumed submit.
  document.addEventListener("submit", function (ev) {
    var f = checkoutForm(ev);
    if (!f || f.querySelector('[name^="updates"]')) return;
    var sub = ev.submitter;
    hold(ev, f, function () { f.requestSubmit ? f.requestSubmit(sub || undefined) : f.submit(); });
  }, true);
  // Classic cart form with quantity fields: they are only sent with this submit, so apply them first,
  // refresh the token for the new contents, then go to checkout. Bubble phase on window, so it runs
  // after the theme's handlers and does nothing if one of them cancelled (terms box, minimum order).
  window.addEventListener("submit", function (ev) {
    var f = checkoutForm(ev);
    if (!f || ev.defaultPrevented || passthrough || !f.querySelector('[name^="updates"]')) return;
    ev.preventDefault();
    origFetch(root + "cart/update.js", { method: "POST", credentials: "same-origin", body: new FormData(f) })
      .catch(function () {})
      .then(function () { dirty = true; return settle(); })
      .then(function () { location.href = root + "checkout"; });
  }, false);

  function maybeRefresh() {
    var s = saved();
    if (document.visibilityState === "visible" && nearExpiry(s) && !running) ensure(true);
  }
  document.addEventListener("visibilitychange", maybeRefresh);
  setInterval(maybeRefresh, 60 * 1000);

  ensure(false);
})();
