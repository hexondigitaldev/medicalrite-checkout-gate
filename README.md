# MedicalRite Checkout Bot Gate

Custom Shopify app that stops card-testing bots at MedicalRite checkout before a card reaches Authorize.net.

- Project context for people and agents: [`CLAUDE.md`](CLAUDE.md)
- Spec (from Timur's brief): [`docs/spec.md`](docs/spec.md)
- Current phase — dev-store spike: [`docs/phase-a-spike.md`](docs/phase-a-spike.md) · [runbook](docs/phase-a-runbook.md) · [findings](docs/phase-a-findings.md)

## Layout

```
extensions/
  probe-validation/   Rust · Cart & Checkout Validation probe (never blocks)
  probe-payment/      Rust · Payment Customization probe (hides nothing by default)
  probe-theme/        Theme app embed · sets a harmless _bg_probe cart attribute
```

Phase B replaces the probes with the real Parts 1–3.
