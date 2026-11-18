# Tests

Résultats observés : [`TEST_REPORT.md`](../TEST_REPORT.md). Ce document décrit comment tester.

## Commandes

```sh
./scripts/test.sh                                   # tous les tests (résumé compact)
cargo test -p aegis-risk                            # un module
cargo test -p aegis-core --test e2e                 # bout en bout (moteur + protocole TCP + simulateur)
cargo test -p aegis-news -- --ignored               # lecture réelle du calendrier (réseau)
cargo test --release -p aegis-core --test capacity -- --ignored --nocapture   # capacité
cd desktop && npm run typecheck                     # interface
```

## Niveaux

| Niveau | Où | Exemples |
|---|---|---|
| Unitaires | `src/` de chaque crate | coûts, objectif, gestion des positions, indicateurs, calibration |
| Propriétés et cas limites | tests unitaires | NaN refusés partout, volumes arrondis vers le bas, stop jamais élargi, pas de regard vers le futur (stratégies, Shadow) |
| Intégration | `crates/*/tests/` | base réelle (fichier), protocole, exécution, API sur vrai TLS |
| Bout en bout | `aegis-core/tests/e2e.rs` | le moteur complet contre le simulateur, par le vrai réseau |
| Injection de pannes | e2e + simulateur (`Fault`) | rejet broker, remplissage partiel, acquittement perdu, requête perdue, terminal non authentifié, rejeu, déconnexion, données périmées, base de données en panne |
| Reprise | e2e | arrêt d'urgence restauré au redémarrage, versions de modèles, paramètres de stratégies, sauvegardes |
| Charge | `aegis-core/tests/capacity.rs` | 2 000 allers-retours |
| Rentabilité | backtest, hors échantillon, walk-forward, Shadow (forward papier), démo | voir `BACKTESTING.md`, `MODEL_VALIDATION.md` |

Le **simulateur** (`aegis-simterminal`) est un adaptateur de test : il parle le vrai protocole
par le vrai réseau mais n'est pas un broker. Il s'annonce `simulator`, ne traite que
`EURUSD.sim`, et ses données sont stockées `synthetic`.

## Ce qu'un test ne doit jamais faire

- dépendre du réseau extérieur (les tests réseau sont `#[ignore]`) ;
- lire un fichier du dépôt à l'exécution (sous macOS 27, la première lecture d'un binaire de
  test neuf échoue parfois avec « No such file ») : utiliser `include_str!` ;
- présenter un résultat sur données synthétiques comme une performance.

## Essais à faire sur Windows (procédure)

1. Pipeline CI (`BUILD_GUIDE.md`) : jobs `test`, `server`, `desktop`, `mql5` verts ;
   télécharger les journaux.
2. Installer `AEGIS_<v>_x64-setup.exe` à la main : l'application s'ouvre sur *Connexion* ;
   « Démarrer le moteur local … avec le simulateur » ; le tableau de bord affiche
   « SIMULATEUR (test) » ; fermer la fenêtre ; vérifier dans le Gestionnaire des tâches
   qu'`aegisd.exe` tourne toujours ; rouvrir : reconnexion automatique.
3. Service : `install-service.ps1` ; `Stop-Process -Name aegisd -Force` ; le service redémarre
   en 5 s, en MONITOR (`GET /api/v1/health`).
4. Désinstaller l'application puis le service ; vérifier que les données restent.
5. Connecteur : `MT5_CONNECTOR.md`.

Reporter chaque observation dans `TEST_REPORT.md`, y compris les échecs.
