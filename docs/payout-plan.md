# Polygon wBTC/USDT Auszahlungskonzept

Dieses Dokument beschreibt das Referenz-Design der beiden Smart-Contract-Rollen:

- **Gemeinschaftspool**: Pool, der durch Notfall- oder Inaktivitäts-Drains befüllt wird und jeden Monat ausgeschüttet wird.
- **Teilnehmer-Plan**: Individueller Auszahlungsplan pro Teilnehmer, der seine Parameter vorgibt und nur an die Ersteller-Wallet auszahlt.

## Token- & Orakel-Annahmen
- Einzahlungen und Auszahlungen erfolgen in **wBTC**.
- Schwellenwert und monatliche Auszahlung werden in **USDT** angegeben und beim Auslösen in wBTC umgerechnet.
- Ein Chainlink-Feed liefert den wBTC/USD-Preis (Standard: 8 Dezimalstellen). Eine einfache USDT = USD Annahme wird verwendet.

## Architektur: zwei getrennte Verträge
- **ParticipantPlan (pro Teilnehmer)**: Custodied das plan-spezifische wBTC-Guthaben, führt Payouts an den Ersteller aus und sendet Anteile an den Pool bei Notfall oder Inaktivität.
- **CommunityPool (gemeinsam)**: Nimmt wBTC aus Notfall-/Inaktivitäts-Drains entgegen und schüttet monatlich aus (1 % Provider, 98 % an nicht-auszahlende Teilnehmer nach Gewicht, 1 % Fee-Buffer).

## Kernregeln pro Plan (ParticipantPlan)
1. **Plan-Erstellung**
   - Constructor zieht `initialDeposit` in wBTC vom Ersteller und speichert:
     - `thresholdUSDT`: USDT-Schwelle, ab der die Auszahlungsphase startet.
     - `monthlyPayoutUSDT`: USDT-Wert einer Monatszahlung, bei Auslösung in wBTC umgerechnet.
   - Weitere wBTC-Einzahlungen sind von beliebigen Wallets möglich.
   - Erreicht die Summe der Einzahlungen den Schwellenwert (Oracle-basiert), wechselt der Plan in die Auszahlungsphase und meldet sich im Pool als nicht-eligible ab.

2. **Monatliche Auszahlungen**
   - Nur die Ersteller-Wallet darf monatliche Auszahlungen triggern (Pull-Modell).
   - Monat = 30 Tage (blocktimestamp-basiert).
   - Payout-Betrag wird beim Auslösen von USDT in wBTC umgerechnet. Ist das Guthaben geringer, wird der Rest ausgezahlt und der Plan geschlossen (und im Pool deregistriert).

3. **Notfallauszahlung**
   - Jederzeit vom Ersteller aufrufbar.
   - 90 % des Plan-Guthabens gehen an den Ersteller, 10 % werden an den CommunityPool gesendet. Der Plan wird geschlossen und im Pool entfernt.

4. **Inaktivität**
   - Nach 90 Tagen ohne Payout kann jeder monatlich einen Drain anstoßen:
     - 10 % des aktuellen Guthabens wandern pro Monat in den CommunityPool, bis Guthaben = 0 oder der Ersteller wieder auszahlt.
   - Sobald Guthaben 0, Plan geschlossen und im Pool deregistriert.

## Gemeinschaftspool-Regeln (CommunityPool)
1. **Registrierung & Eligibility**
   - Plans registrieren sich beim Pool mit ihrer Ersteller-Adresse und Gewicht (= Initialeinzahlung).
   - Wird ein Plan auszahlungsreif, meldet er sich als nicht-eligible; bei Schließung entfernt er sich vollständig.
2. **Ausschüttung**
   - Funktion `distribute(candidatePlans[])` verteilt das angesammelte Pool-Guthaben:
     - 1 % an Provider-Wallet.
     - 98 % an Pläne, die noch nicht in der Auszahlungsphase sind (Gewichtung nach Initialeinzahlung).
     - 1 % verbleibt als Fee-Buffer im Pool.
   - Die Kandidatenliste begrenzt die Gas-Kosten und wird off-chain zusammengestellt.

## Datenmodell (Solidity)
- `ParticipantPlan`
  - `creator`, `initialDeposit`, `totalDeposits`, `escrowBalance`
  - `thresholdUSDT`, `monthlyPayoutUSDT`
  - `lastPayoutAt`, `lastDepositAt`, `inactiveDrainStart`
  - `inPayoutPhase`, `closed`
- `CommunityPool`
  - `plans[plan] = {creator, weight, eligible}`
  - `communityBalance`: WBTC-Betrag, der auszuschütten ist (Rest 1 % verbleibt)

## Sicherheit & Grenzen
- Pull-basiertes Auslösen reduziert Automatisierungsrisiken.
- Monatliche Dauer = 30 Tage; reale Kalendermonate erfordern ggf. Off-Chain-Scheduler.
- Orakel-Staleness-Check (max. 1 Tag alt) verhindert alte Preise.
- Kein Reentrancy-Guard in der Referenz (für Produktion empfohlen).
- Die Kandidatenliste für Pool-Ausschüttungen muss off-chain kuratiert werden, um Gas zu begrenzen.

## Erweiterungsmöglichkeiten
- **Plan-Factory + Upgradebarkeit**: Eine Factory erzeugt dedizierte Plan-Instanzen; ein UUPS/Transparent-Proxy sichert zukünftige Upgrades (z. B. neue Tokens, andere Gebührenlogik).
- **Preis-Fallbacks**: Neben Chainlink-Feed optionaler DEX-TWAP (z. B. Uniswap V3) als Backstop; Guardrails wie Mindest-Liquidität und Max-Drift gegenüber Hauptorakel.
- **Multi-Stablecoin**: Unterstützung weiterer Stablecoins (USDC/DAI) mit einheitlichem 6-Decimals-Interface und token-spezifischen Preisfeeds.
- **Automatisierung**: Chainlink Automation/gelernte Keeper-Services zum periodischen Auslösen von Monats-Payouts und Inaktivitäts-Drains, inkl. Pausier-Flag für Wartung.
- **Sicherheitsmodule**: ReentrancyGuards, pausierbare Funktionen, Rate-Limits für Emergency/Drain-Aufrufe und Rolling-Orakel-Drift-Checks.
- **Analytics & Off-Chain-Indexing**: The Graph/Substreams für Events (PayoutTriggered, EmergencyExit, CommunityDistributed) zur Abrechnung und UI-Visualisierung.
