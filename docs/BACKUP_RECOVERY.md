# Sauvegarde et reprise

## Ce qu'il faut sauvegarder

| Élément | Pourquoi |
|---|---|
| `data/aegis.db` | Tout l'historique : décisions, ordres, clôtures, audit, limites de risque, arrêt d'urgence |
| `config/aegis.toml` | Votre configuration |
| `data/shared.key` (ou la variable `AEGIS_SHARED_KEY`) | Sans elle il faut ressaisir une clé dans MT5 |
| `models/` | Modèles entraînés |

## Sauvegarder

- **Depuis l'application** (moteur en marche) : *Surveillance du système → Créer une
  sauvegarde*. Le fichier `data/backups/aegis-<date>.db` est une copie cohérente
  (`VACUUM INTO`), prise sans arrêter le moteur.
- **En ligne de commande** (moteur arrêté) : `./scripts/backup.sh`.
- Ne copiez pas `aegis.db` à la main pendant que le moteur tourne : sans ses fichiers `-wal`
  et `-shm` la copie peut être incomplète.

Copiez régulièrement `data/backups/` vers un autre disque ou un autre ordinateur.

## Restaurer

1. **Arrêtez** l'application ou `aegisd`.
2. Lancez :
   ```sh
   target/release/aegisd --config config/aegis.toml restore --from data/backups/aegis-2026-11-15-12-00-00.db
   ```
   La commande vérifie d'abord la sauvegarde (intégrité SQLite + chaîne du journal d'audit) et
   **refuse** un fichier corrompu. L'ancienne base est conservée sous
   `data/aegis.db.before-restore`.
3. Relancez. Le moteur démarre en MONITOR.

## Vérifier l'intégrité

```sh
target/release/aegisd --config config/aegis.toml verify-audit
```

ou *Surveillance du système → Vérifier l'intégrité du journal d'audit*. Le journal est en ajout
seul (les modifications et suppressions sont refusées par la base) et chaque entrée contient
l'empreinte de la précédente : une entrée modifiée est détectée et désignée.

## Reprise après un arrêt inattendu

Rien à faire de particulier : relancez l'application.

| Situation au moment de la coupure | Au redémarrage |
|---|---|
| Mode DEMO AUTO ou LIVE AUTO | Retour en **MONITOR** ; à réactiver à la main |
| Arrêt d'urgence actif | **Toujours actif** |
| Ordre envoyé, réponse non reçue | Marqué `unknown` ; dès que le terminal renvoie l'état du compte, il devient `filled` (position ou clôture retrouvée) ou `not_executed` |
| Position ouverte | Elle reste chez le broker avec son SL et son TP ; AEGIS la retrouve à la reconnexion |
| Limites de risque modifiées | Conservées (la dernière version enregistrée est rechargée) |
| Écriture en cours | Annulée proprement (transactions SQLite, mode WAL) |

Les positions sont toujours protégées **chez le broker** par leur stop-loss : elles ne
dépendent pas d'AEGIS pour être stoppées.

## Si la base est endommagée

1. Arrêtez AEGIS. Mettez de côté `data/aegis.db*`.
2. Restaurez la dernière sauvegarde saine (ci-dessus).
3. Sans sauvegarde : renommez `data/aegis.db` ; AEGIS en recrée une vide au démarrage.
   L'historique est perdu ; les positions restent chez le broker et réapparaissent à la
   connexion (comme positions dont AEGIS ne connaît plus l'origine).

## Conservation

Les bougies de plus de `retention.candle_days` jours et les instantanés de compte de plus de
`retention.snapshot_days` jours sont supprimés une fois par jour. Les décisions, ordres,
clôtures et le journal d'audit ne sont **jamais** supprimés.
