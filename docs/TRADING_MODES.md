# Modes de fonctionnement

AEGIS a trois modes. Le moteur **démarre toujours en MONITOR**, y compris après une panne,
une mise à jour ou une réinitialisation de l'arrêt d'urgence : aucun mode automatique n'est
jamais restauré.

| Mode | Ce qui se passe | Conditions |
|---|---|---|
| **MONITOR** | Marché analysé, stratégies évaluées, probabilités et décisions calculées et expliquées. **Aucune trame d'ordre ne sort du moteur**, pas même une clôture manuelle. | Toujours disponible. |
| **DEMO AUTO** | Les décisions approuvées sont exécutées automatiquement. | Terminal connecté sur un compte de **démonstration** (ou le simulateur de test) ; stratégie au stade Démo ou Approuvée réel. |
| **LIVE AUTO** | Exécution automatique sur un compte **réel**. | `safety.live_trading_enabled = true` dans le fichier de configuration du serveur, compte réel, stratégie au stade Approuvée réel, **et** armement explicite : `<numéro de compte> J'ACCEPTE LE RISQUE REEL`. |

LIVE AUTO est désactivé par défaut. Aucun ordre réel ne part sans ces trois verrous.

## Ce qui bloque toute nouvelle entrée, quel que soit le mode

- arrêt d'urgence actif (manuel, ou automatique au drawdown maximal) ;
- terminal déconnecté, prix ou données de compte périmés, données de marché incohérentes ;
- ordre au sort inconnu sur l'instrument (jusqu'à la réconciliation) ;
- base de données indisponible (une décision non enregistrée ne peut jamais produire d'ordre) ;
- perte journalière maximale, objectif du jour atteint (selon sa politique), nombre maximal de
  transactions, pertes consécutives (pause), débit d'ordres, horaires, exposition ;
- actualité à fort impact sur une devise de l'instrument ;
- probabilité calibrée absente ou sous le seuil BUY/SELL, espérance nette insuffisante ;
- coûts du jour supérieurs à la part autorisée des gains bruts.

Chaque refus est enregistré avec le contrôle en cause et son détail (page Décisions).

## Arrêt d'urgence

- Engagé depuis l'application (bouton rouge), l'API ou automatiquement au drawdown maximal.
- Effet immédiat : plus aucune entrée. Politique configurable pour l'existant : annuler les
  ordres en attente d'AEGIS ; conserver les positions avec leurs stops (défaut) ou les clôturer.
  Les positions ouvertes hors d'AEGIS ne sont jamais touchées.
- Il survit à un redémarrage. Réinitialisation manuelle avec la phrase `RESET AEGIS` ; le
  moteur repasse alors en MONITOR.

## Stades des stratégies

`Recherche → Démo → Approuvée réel` (et `Désactivée`). Passer en Démo exige au moins 30
transactions hors échantillon au résultat net positif sur données **broker** ; passer en
Approuvée réel exige en plus des résultats forward sur le compte démo. Modifier les paramètres
d'une stratégie la renvoie en Recherche. Un challenger du Shadow ne peut être promu que vers la
Démo, par une commande explicite.
