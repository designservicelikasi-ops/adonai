# AEGIS — Adaptive Engine for Intelligent Strategies

Plateforme de trading automatisé et de gestion du risque pour MetaTrader 5, orientée
**micro-gains à espérance nette positive** : beaucoup de petites transactions, chacune validée
indépendamment par sa probabilité calibrée d'atteindre la cible avant le stop, ses coûts réels
et les limites de risque — ou pas de transaction du tout.

> AEGIS ne promet aucune rentabilité. Aucune stratégie ni aucun modèle livré n'a encore été
> validé sur des données de marché réelles. Le trading réel (LIVE AUTO) est désactivé par
> défaut. État détaillé : [`PROJECT_STATUS.md`](PROJECT_STATUS.md).

## Composants

| Composant | Quoi | Où |
|---|---|---|
| **AEGIS ENGINE** | serveur Rust autonome (`aegisd`) : marché, 9 stratégies, Probability Engine, risque, objectif quotidien, gestion des positions, exécution, journal, actualités, Shadow, API TLS, service Windows/systemd | `crates/` |
| **AEGIS CONNECTOR** | Expert Advisor MQL5 : données et exécution authentifiée | `mt5/` |
| **AEGIS DESKTOP** | application Tauri 2 (React/TypeScript) de supervision et de contrôle, locale ou vers un VPS | `desktop/` |

## Démarrage rapide (sans MetaTrader : simulateur de test)

```sh
./scripts/setup.sh          # compile le moteur, puis l'application si Node.js est présent
./scripts/run.sh            # ouvre AEGIS Desktop
```

Page *Connexion* → « Démarrer le moteur local … avec le simulateur de test ». Tout ce que
produit le simulateur est fictif et signalé comme tel.

Moteur seul : `./scripts/run-daemon.sh [--simulator]`, puis `aegisd pairing` pour connecter
le desktop.

## Documentation

| Pour… | Lire |
|---|---|
| Utiliser l'application | [`docs/USER_MANUAL.md`](docs/USER_MANUAL.md), [`docs/TRADING_MODES.md`](docs/TRADING_MODES.md) |
| Installer | [`docs/INSTALLATION_WINDOWS.md`](docs/INSTALLATION_WINDOWS.md), [`docs/INSTALLATION_LINUX.md`](docs/INSTALLATION_LINUX.md), [`docs/VPS_DEPLOYMENT.md`](docs/VPS_DEPLOYMENT.md), [`docs/MT5_CONNECTOR.md`](docs/MT5_CONNECTOR.md) |
| Configurer | [`docs/CONFIGURATION.md`](docs/CONFIGURATION.md), [`docs/RISK.md`](docs/RISK.md), `config/aegis.example.toml` |
| Comprendre | [`ARCHITECTURE.md`](ARCHITECTURE.md), [`DECISIONS.md`](DECISIONS.md), [`docs/MODEL_VALIDATION.md`](docs/MODEL_VALIDATION.md), [`docs/BACKTESTING.md`](docs/BACKTESTING.md) |
| Sécurité | [`docs/SECURITY.md`](docs/SECURITY.md) |
| Intégrer | [`docs/API_PROTOCOL.md`](docs/API_PROTOCOL.md), [`docs/MT5_PROTOCOL_V1.md`](docs/MT5_PROTOCOL_V1.md) |
| Développer, compiler, tester | [`docs/DEVELOPER_MANUAL.md`](docs/DEVELOPER_MANUAL.md), [`docs/BUILD_GUIDE.md`](docs/BUILD_GUIDE.md), [`docs/TESTING.md`](docs/TESTING.md), [`TEST_REPORT.md`](TEST_REPORT.md) |
| Dépanner | [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md), [`docs/BACKUP_RECOVERY.md`](docs/BACKUP_RECOVERY.md) |
| Limites | [`KNOWN_LIMITATIONS.md`](KNOWN_LIMITATIONS.md) |
| Historique | [`CHANGELOG.md`](CHANGELOG.md), spécification : [`docs/SPECIFICATION.md`](docs/SPECIFICATION.md) |

## Principes

- **MONITOR au démarrage, toujours.** DEMO AUTO exige un compte de démonstration ; LIVE AUTO
  exige d'être autorisé dans la configuration, une stratégie approuvée et une phrase de
  confirmation avec le numéro du compte.
- **Une probabilité non validée n'existe pas.** Pas de probabilité calibrée hors échantillon
  ⇒ pas d'entrée. Les seuils de l'utilisateur ne sont jamais abaissés automatiquement.
- **Le net, pas le brut.** Spread, commission, slippage, swap et frais sont estimés avant
  chaque ordre et mesurés après ; brut et net ne sont jamais confondus.
- **Le broker fait foi.** Un état inconnu n'est jamais pris pour une absence d'exécution.
- **Rien d'inventé.** Une valeur indisponible s'affiche comme telle ; le simulateur est
  signalé partout ; ce qui n'a pas été testé est écrit « NON VÉRIFIÉ ».
