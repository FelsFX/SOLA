# SOLA Auszahlung dApp

Dieses Repository enthält das Anchor-Programm `payout_plan`, das einen automatisierten Auszahlungsplan
für tBTC-Einzahlungen abbildet. Die monatlichen Schwellenwerte werden dabei in USDT mit sechs Nachkommastellen
verwaltet und dynamisch in tBTC umgerechnet.

## Aufbau

- `Anchor.toml` – Anchor Workspace Konfiguration
- `Cargo.toml` – Rust Workspace Konfiguration
- `programs/payout_plan` – Anchor Programm mit Geschäftslogik und Komponententests

## Tests

Die Logik lässt sich lokal ohne Validator testen:

```bash
cargo test
```
