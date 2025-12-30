# SOLA

Referenz-Spezifikation und Beispiel-Smart-Contracts für wBTC/USDT-Auszahlungspläne auf Polygon.

- `contracts/ParticipantPlan.sol`: Per-Teilnehmer-Vertrag, der Einzahlungen hält, monatliche Payouts steuert und Drains/Notfälle in den Pool sendet.
- `contracts/CommunityPool.sol`: Gemeinsamer Pool, der wBTC aus Drains/Notfällen sammelt und an nicht-auszahlende Pläne + Provider verteilt.
- `docs/payout-plan.md`: Prozess- und Regelbeschreibung der Aufteilung in Plan- und Pool-Vertrag.
- `frontend/index.html`: Leichtgewichtige, rein lokale Demo-Oberfläche zur Simulation von Plan/Payout/Pool-Flows.
