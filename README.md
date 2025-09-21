# SOLA Auszahlung dApp

Dieses Repository enthält ein Solana/Anchor-Programm, das einen automatisierten Auszahlungsplan für Einzahler:innen abbildet. Kernidee ist, dass alle Zahlungen ausschließlich in tBTC abgewickelt werden, während die ursprüngliche Planung und Schwellenwerte in USDT (6 Nachkommastellen) gepflegt werden.

## Funktionsumfang

- **Initialisierung eines Plans**: Beim Anlegen eines Plans legt die Nutzerin bzw. der Nutzer die monatliche Auszahlung in USDT fest und hinterlegt Metadaten für das begleitende NFT.
- **Automatisches NFT-Onboarding**: Bei der ersten Einzahlung wird die hinterlegte NFT-Mint-Adresse gespeichert. Das eigentliche Minting erfolgt im Client, aber der Programmspeicher zeichnet Status und Metadaten nach.
- **Startschwelle**: Auszahlungen beginnen automatisch, sobald das eingezahlte Guthaben mindestens dem 100-fachen der Monatsrate entspricht.
- **Dynamischer Auszahlungsplan**: Nach dem Start steigen die monatlichen Auszahlungen exponentiell mit dem Faktor `1,003726`.
- **tBTC-Auszahlung**: Die tatsächlich auszuzahlende tBTC-Menge wird anhand eines extern bereitgestellten USDT/tBTC-Wechselkurses berechnet.
- **Notfalllogik**: Über das NFT (bzw. denselben Wallet-Owner) kann ein Notfallmodus ausgelöst und anschließend eine vollständige Auszahlung des Restguthabens vorgenommen werden.

## Projektstruktur

```
├── Anchor.toml
├── Cargo.toml
├── programs
│   └── payout_plan
│       ├── Cargo.toml
│       └── src
│           └── lib.rs
└── README.md
```

## Wichtige Konstanten

| Konstante | Beschreibung |
|-----------|--------------|
| `START_THRESHOLD_MULTIPLIER` | Faktor (100×) für die Aktivierung des Plans. |
| `GROWTH_FACTOR_NUMERATOR` / `GROWTH_FACTOR_DENOMINATOR` | Fixpunkt-Faktor 1,003726 zur monatlichen Steigerung. |
| `TBTC_DECIMALS` | 10⁸, um tBTC-Mengen in der typischen Solana-Darstellung abzubilden. |
| `USDT_DECIMALS` | 10⁶, Standard-Skalierung für USDT. |

## Events

- `PlanInitialized`
- `PlanFunded`
- `PayoutExecuted`
- `EmergencyTriggered`
- `EmergencyWithdrawal`

Diese Events können vom Client genutzt werden, um den aktuellen Status des Plans zu verfolgen.

## Tests ausführen

Das Repository enthält rein logische Komponententests, die ohne Solana-Validator lauffähig sind:

```bash
cargo test
```

Die Tests validieren insbesondere die Wachstums- und Umrechnungslogik.

## Weiterführende Schritte

- Integration eines Preis-Orakels (z. B. Pyth) für `price_usdt_per_tbtc`.
- On-Chain Minting/Verwaltung des NFTs (z. B. via Metaplex Token Metadata Program).
- Client-seitige UI zur Verwaltung von Einzahlungen, Auszahlungen und dem Notfall-Workflow.
