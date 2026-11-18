# Connecteur MetaTrader 5 (AEGIS CONNECTOR)

Fichiers : `mt5/AegisConnector.mq5` (Expert Advisor), `mt5/AegisProtocol.mqh` (trames, JSON
plat, SHA-256/HMAC), `mt5/AegisSelfTest.mq5` (script de vérification). Protocole :
`MT5_PROTOCOL_V1.md` (version 1.1 compatible : frais, devise de base, niveau de gel).

> **État : jamais compilé ni exécuté sur la machine de développement (macOS, pas de
> MetaEditor).** Le pipeline CI tente la compilation sur Windows (job `mql5`). Tant que cette
> compilation et un essai sur compte démo n'ont pas été observés, le connecteur est
> **NON VÉRIFIÉ**.

## Rôle

Le connecteur ne décide jamais de trader. Il :

- se connecte au moteur (le moteur est le serveur ; MT5 n'autorise que les sorties), s'authentifie
  (poignée de main HMAC, clé de session) ;
- envoie : spécifications des instruments, ticks, bougies d'historique, compte, instantanés de
  positions/ordres, transactions clôturées, opérations de solde ;
- exécute **uniquement** les instructions authentifiées, non expirées, non rejouées, dont le
  volume est valide et dont le prix n'a pas trop bougé depuis l'autorisation ;
- rapporte la réponse exacte du broker (code retour, prix, volume, éventuel remplissage partiel).

## Installation

1. Copier les trois fichiers dans `MQL5/Experts/AEGIS/` du terminal
   (*Fichier → Ouvrir le dossier des données*).
2. Ouvrir `AegisConnector.mq5` et `AegisSelfTest.mq5` dans MetaEditor, compiler (F7).
   Attendu : `0 errors`. Signaler toute erreur (nom de fonction, conversion de type) : le
   code a été écrit d'après la référence MQL5 sans compilateur.
3. Lancer le script **AegisSelfTest** sur un graphique : il doit afficher `ALL PASSED`
   (vecteurs SHA-256/HMAC identiques à ceux du moteur, analyse JSON).
4. *Outils → Options → Expert Advisors* : cocher « Autoriser le trading algorithmique » et
   ajouter l'adresse du moteur (`127.0.0.1`) dans la liste des adresses autorisées.
5. Glisser **AegisConnector** sur un graphique de chaque instrument à suivre (ou un seul
   graphique avec `InpSymbols`). Paramètres :

| Paramètre | Valeur |
|---|---|
| `InpHost` / `InpPort` | adresse et port du moteur (`127.0.0.1` / `7878`) |
| `InpSharedKey` | contenu de `<data_dir>/shared.key` du moteur (ou `AEGIS_SHARED_KEY`) |
| `InpSymbols` | instruments séparés par des virgules ; vide = celui du graphique |
| `InpMagic` | identique à `execution.magic` du moteur (20260115) |
| `InpAllowRealAccount` | `false` : refuse toute NOUVELLE entrée sur un compte réel (double verrou avec LIVE AUTO) |
| `InpTimerMs` | période de scrutation (100 ms) |

## Vérification sur compte démo (procédure)

1. Moteur en MONITOR, connecteur attaché : AEGIS Desktop doit afficher « MT5 connecté », le
   serveur, le numéro de compte, le type `DEMO`, les prix et le spread ; la page *Système*
   montre le nombre de trames.
2. Vérifier dans le journal du moteur : spécification reçue, historique reçu pour chaque unité
   de temps, aucun refus de poignée de main.
3. Entraîner le modèle de probabilité sur l'historique **broker** collecté, lancer les
   backtests, promouvoir une stratégie en Démo si ses preuves le permettent.
4. Passer en DEMO AUTO pendant une séance. Vérifier pour chaque transaction : décision
   enregistrée, ordre `filled` avec le volume calculé, position visible avec SL/TP, gestion
   (SECURE/trailing) reflétée dans MT5, clôture reportée, résultat net identique à celui du
   terminal, journal et performance à jour.
5. Couper le terminal pendant qu'une position est ouverte : entrées bloquées, alerte ; au
   retour, réconciliation sans doublon.
6. Reporter les observations dans `TEST_REPORT.md`.

## Limites

Comptes « hedging » visés (le cas « netting » repose sur un repli non testé) ; commission non
fournie par l'API MT5 (à saisir dans la configuration de risque) ; liaison authentifiée mais
non chiffrée (rester sur la boucle locale ou un tunnel).
