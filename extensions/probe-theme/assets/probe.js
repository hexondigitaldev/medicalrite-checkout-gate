// Phase A probe. Logs to the browser console with prefix [bot-gate-probe].
(function () {
  var root = (window.Shopify && window.Shopify.routes && window.Shopify.routes.root) || "/";
  function log() {
    var args = Array.prototype.slice.call(arguments);
    args.unshift("[bot-gate-probe]");
    console.log.apply(console, args);
  }
  async function run() {
    var cart = await (await fetch(root + "cart.js", { credentials: "same-origin" })).json();
    log("cart token", cart.token, "items", cart.items.map(function (i) { return i.key; }), "attributes", cart.attributes);
    var current = cart.attributes && cart.attributes._bg_probe;
    var wanted = "probe:" + cart.token;
    if (current !== wanted) {
      await fetch(root + "cart/update.js", {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ attributes: { _bg_probe: wanted } }),
      });
      log("set _bg_probe =", wanted);
    }
  }
  run().catch(function (e) { log("error", e); });
  // Re-run after add-to-cart (cart token can change when a cart is first created).
  document.addEventListener("submit", function () { setTimeout(function () { run().catch(function () {}); }, 1500); }, true);
})();
