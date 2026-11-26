# AEGIS AUTONOMY COMMANDER

Le Commander décide **quelles opérations AEGIS peut mener seul**. Il ne juge jamais une
transaction : le **Risk Guardian** garde un droit de veto indépendant sur chaque ordre, quelle
que soit la permission. « Carte blanche » signifie l'autonomie maximale **dans un périmètre
explicitement accordé**, jamais un pouvoir illimité sur le compte.

Code : `crates/aegis-autonomy` (règles pures, testées), `crates/aegis-core/src/autonomy.rs`
(intégration), tables `permission_grants` et `permission_events` (migration 0007, retour
arrière : `crates/aegis-data/migrations_down/0007_autonomy.down.sql`).

## Capacités (18)

Lire les données MT5, analyser, lancer les stratégies, évaluer les probabilités, backtests,
entraînement, proposition de stratégies, opérations de démonstration, ouvrir, fermer, modifier
SL/TP, gérer le portefeuille, Shadow, rapports, redémarrage des services, surveillance de la
connexion, alertes, sauvegardes. Les cinq capacités de trading envoient des ordres.

## Profils

| Profil | Ce qu'AEGIS fait seul | Mode | Accord |
|---|---|---|---|
| OBSERVER | analyse, affichage, alertes, sauvegardes | MONITOR | par défaut possible |
| RESEARCH AUTONOMY (défaut) | + entraînement, backtests, comparaisons, Shadow, propositions | MONITOR | par défaut possible |
| DEMO FULL AUTONOMY | + ouvrir, fermer, modifier, gérer — sur un compte DÉMO | DEMO AUTO | autorisation liée au compte |
| LIVE AUTHORIZED AUTONOMY | + trading sur un compte RÉEL identifié | LIVE AUTO | autorisation explicite, datée |

Aucun profil de trading ne peut être le profil par défaut.

## Autorisations (grants)

Une autorisation est **toujours liée à un compte** (numéro et serveur). Elle cesse
immédiatement de s'appliquer — et le moteur repasse en MONITOR — quand :
- elle expire, ou elle est révoquée (`revoke_autonomy`, `set_mode monitor`, réinitialisation
  de l'arrêt d'urgence) ;
- un autre compte se connecte ;
- (LIVE) la **configuration de risque change** : l'autorisation est liée à son empreinte ;
- (LIVE) `safety.live_trading_enabled` passe à false, ou le compte n'est plus un compte MT5 réel.

LIVE exige en plus : la configuration, un vrai compte MT5 réel, au moins une stratégie
approuvée pour le réel, une **durée** (7 jours au plus ; `set_mode live_auto` utilise
`autonomy.live_default_hours`) et la phrase `<compte> J'ACCEPTE LE RISQUE REEL`.

**Personne d'autre que l'utilisateur ne peut accorder une autorisation.** Seul le canal des
commandes utilisateur construit la preuve `UserConfirmation` ; un test d'architecture
(`crates/aegis-core/tests/architecture.rs`) échoue si un autre fichier le fait.

**Redémarrage.** Le moteur démarre toujours en MONITOR. Seule une autorisation DÉMO marquée
`persist_across_restart` est reprise, et seulement quand le même compte se reconnecte (utile
sur un VPS). Une autorisation LIVE n'est jamais restaurée.

## Actions de protection

Réinstallation d'un stop, clôture de protection, plan de l'arrêt d'urgence : ces actions
**réduisent** le risque et ne sont jamais bloquées par une permission. Elles sont tout de
même inscrites dans le journal des permissions (`risk_guardian`).

## Journal des permissions

Chaque événement : qui, quoi, quel compte, quand, quel moteur l'a demandé, résultat.
Événements : `grant`, `revoke`, `lapse`, `resume`, `default_profile` (aussi dans l'audit
chaîné), `action` (ordre envoyé par un moteur autonome), `denied` (refus).
