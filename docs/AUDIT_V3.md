# AEGIS — Audit 0.2 → plan 0.3 (directive « Master Upgrade », 2026-11-26)

Ce document est l'audit de départ de la version 0.3 et le plan suivi. Chaque affirmation sur
l'existant a été vérifiée dans le code ou par exécution le 2026-11-26.

## 1. État initial constaté

- Dépôt : `main` = `origin/main` (`8a96ddb`), 26 commits. Les 209 fichiers « modifiés » ne
  l'étaient que par leurs droits (644 → 755, disque exFAT) : `core.filemode=false` a été posé
  localement, aucun contenu n'a changé. Fichiers `._*` (métadonnées macOS) ignorés.
- Outils : Rust 1.100, Node 22, Python 3.9 + uv. Pas de `gh`, pas de MetaEditor, pas de MT5.
- **Tests de référence : 241 réussis, 0 échoué** (`./scripts/test.sh`, 61 s).

### Ce qui existe et fonctionne (vérifié par les tests)

| Domaine | Existant | Où |
|---|---|---|
| Garde-fous de risque | 30+ contrôles expliqués par décision : perte du jour, drawdown, taille au risque, refus si volume minimum > budget, spread, marge, exposition notionnelle et par devise, débit, pertes consécutives + pause, coûts/gains, horaires, fraîcheur prix/compte | `aegis-risk/src/engine.rs` |
| Autorisations | jeton HMAC signé par le seul moteur de risque, 3 s, usage unique | `aegis-risk/src/auth.rs` |
| Stop | SL jamais élargi (exécuteur et Position Intelligence), break-even + coûts, suiveur ATR, distance et gel broker | `aegis-execution`, `aegis-risk/src/position.rs` |
| Exécution | idempotence (cid), délais, état inconnu → réconciliation par instantané, partiels, rejets | `aegis-execution` |
| Probabilités | P(TP avant SL) triple barrière, walk-forward purgé, Platt, Brier/log loss/fiabilité, versions, retour arrière | `aegis-probability` |
| Cerveau micro-profits | cible dans une fourchette de gain NET, leçons de frais/calibration, réentraînement proposé (jamais activé seul) | `aegis-brain`, `aegis-core/src/brain.rs` |
| Shadow | tâche séparée sans accès à l'exécution, champion/challenger papier | `aegis-shadow` |
| Modes | MONITOR au démarrage, DEMO AUTO refusé sur compte réel, LIVE AUTO = config + compte réel + phrase liée au compte, jamais restauré | `aegis-core/src/command.rs` |
| Persistance | SQLite WAL, audit chaîné SHA-256, décision écrite avant tout envoi | `aegis-data` |
| API / Desktop | Axum + TLS épinglé + jeton ; Tauri 2, 11 pages (thème sombre) | `aegisd`, `desktop/` |

### Écarts avec la directive (ce qui manque réellement)

| Partie | Écart |
|---|---|
| 13 Risk Guardian | pas de **risque de portefeuille cumulé** (somme des pertes aux stops), pas de jours autorisés, pas de budget de risque journalier, pas de réduction du risque après pertes, pas de détection de **SL manquant** ni de réaction d'urgence |
| 6.2 / 14 | **mode de compte (hedging / netting) inconnu du moteur** : ni le protocole ni le connecteur ne le transmettent (limite connue n° 9) |
| 4.1 | stop = distance de la stratégie ; aucun choix **structurel / volatilité** |
| 4.2 | la modification d'un SL n'est pas **vérifiée** dans l'instantané broker suivant |
| 14 | latence : seul l'aller-retour est mesuré ; pas de séparation moteur / réseau+broker |
| 7 | aucun **Autonomy Commander** : pas de capacités, profils, octrois datés et révocables, journal des permissions |
| 2.2 / 2.3 | 5 régimes ; pas de M30/H4/D1 construits ; pas d'analyse **multi-unités** |
| 3 | aucun **scanner d'opportunités** ni détection de structure (swings, BOS, CHoCH, FVG…), pas de score distinct de la probabilité |
| 6 | aucun moteur de **re-entry / couverture** ; pas d'analyse de récupération |
| 8 | entraînement mono-fil ; pas de balayage de paramètres ni de détection de sur-apprentissage |
| 11 | pas de **corrélations** entre instruments dans l'exposition |
| 16 | thème sombre, 11 pages ; directive : **blanc et or**, 23 pages |
| 17 | pas de notification externe (Telegram) |

## 2. Plan suivi (ordre de la partie 23)

Chaque lot : compilation, tests, documentation, commit.

1. **Phase 1** — audit (ce document), stabilisation du dépôt.
2. **Phase 2 — Risk Guardian 2.0 et exécution** : mode de compte (protocole 1.2 + connecteur),
   risque de portefeuille, jours autorisés, budget de risque journalier, réduction du risque
   après pertes (jamais d'augmentation), garde de protection (SL manquant / modification non
   constatée), planificateur de stop (stratégie / structure / volatilité, volume recalculé),
   latence séparée.
3. **Phase 3 — Autonomy Commander** : crate `aegis-autonomy` (capacités, 4 profils, octrois
   liés au compte, durée, révocation), migration réversible, journal des permissions,
   intégration aux modes existants.
4. **Phase 4 — Régimes, multi-unités, structure, scanner** : régimes détaillés, M30/H4/D1,
   lecture multi-unités avec contradictions, détecteurs de structure testés, scanner continu
   avec score explicable séparé de la probabilité.
5. **Phase 5 — Micro-profit 2.0 et récupération** : faisabilité par objectif, garde de
   re-entry anti-martingale, gestionnaire de couverture selon le mode du compte, analyses.
6. **Phase 6 — Apprentissage rapide** : entraînements et backtests parallèles bornés,
   balayage de paramètres avec stabilité et sur-apprentissage, retour arrière automatique.
7. **Phase 7 — Multi-actifs** : corrélations et exposition globale.
8. **Phase 8 — Desktop blanc et or**, pages reliées au moteur.
9. **Phase 9** — validation, documentation, rapport final.

## 3. Règles tenues pendant tout le travail

- Aucune protection n'est assouplie ; toute nouvelle règle ne peut que **refuser** ou
  **réduire**. Les valeurs par défaut des nouvelles limites n'interdisent rien de ce qui
  passait déjà, sauf ce qui était un risque non mesuré (position sans stop).
- Les nouvelles colonnes et tables sont ajoutées par migrations ; chaque migration a son
  script de retour arrière dans `crates/aegis-data/migrations_down/`.
- Les champs de protocole ajoutés sont optionnels : un connecteur 1.1 reste accepté.
- Ce qui ne peut pas être vérifié ici (MT5 réel, Windows, MetaEditor) est marqué NON VÉRIFIÉ.
