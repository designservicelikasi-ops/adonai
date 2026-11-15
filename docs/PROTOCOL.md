# AEGIS PROTOCOL v1

Protocole entre le moteur (Rust) et le connecteur MetaTrader 5 (MQL5).
Code de référence : `crates/aegis-protocol/` (moteur) et `mt5/AegisProtocol.mqh` (connecteur).

> **État de vérification.** Le côté Rust est testé (16 tests de protocole, 4 tests de
> compatibilité MQL5, tests de bout en bout avec le simulateur, qui parle ce même protocole
> par le réseau). Le côté MQL5 est écrit mais **n'a jamais été compilé** : voir
> `KNOWN_LIMITATIONS.md`, point 1.

## 1. Transport

- TCP. **Le connecteur est le client, le moteur est le serveur** (MT5 n'accepte que des
  connexions sortantes vers des adresses autorisées).
- Adresse d'écoute : `[bridge] listen` dans la configuration (par défaut `127.0.0.1:7878`).
- **Un seul terminal à la fois.** Une seconde connexion est fermée immédiatement et
  consignée.
- **La liaison est authentifiée mais pas chiffrée.** Prix et ordres circulent en clair :
  gardez la liaison sur une seule machine ou dans un tunnel privé (WireGuard, SSH). Voir
  `docs/DEPLOYMENT.md`.

## 2. Format d'une trame

Une trame par ligne, en UTF-8, terminée par `\n` :

```
<64 caractères hexadécimaux : HMAC-SHA256(clé, json)> <espace> <objet JSON> \n
```

- Le HMAC porte sur **la chaîne JSON exacte** qui suit l'espace : aucune remise en forme à
  faire d'un côté ou de l'autre.
- L'objet JSON est **plat** (aucun objet ni tableau imbriqué), pour que le connecteur puisse
  le lire avec un analyseur minimal.
- Taille maximale d'une trame : **16 384 octets**. Au-delà, la connexion est fermée ; un pair
  ne peut donc pas faire grossir la mémoire du moteur.
- Le HMAC est vérifié **avant** toute lecture du contenu.

Exemple (HMAC raccourci) :

```
3f9a…c41e {"v":1,"id":"sid-7c2e-12","ts":1800000000000,"seq":12,"sid":"sid-7c2e9b…","t":"tick","symbol":"EURUSD","time":1800000000000,"bid":1.10012,"ask":1.10020}
```

## 3. Enveloppe

Champs présents dans toutes les trames :

| Champ | Type | Rôle |
|---|---|---|
| `v` | entier | Version du protocole. Vaut `1`. Toute autre valeur est refusée. |
| `id` | texte | Identifiant du message (traçabilité). |
| `ts` | entier | Horodatage d'émission, millisecondes UTC. |
| `seq` | entier | Numéro de séquence dans la session, par sens. `0` pendant la poignée de main, puis 1, 2, 3… |
| `sid` | texte | Identifiant de session. Vide dans `hello`. |
| `t` | texte | Type du message (voir § 6). |

Les autres champs dépendent du type.

## 4. Poignée de main

```
connecteur → moteur : hello    HMAC avec la clé partagée   (porte un nonce client)
moteur → connecteur : welcome  HMAC avec la clé partagée   (porte l'identifiant de session et un nonce serveur)
ensuite             : toutes les trames utilisent la clé de session
```

Clé de session :

```
clé_session = HMAC-SHA256(clé_partagée, "AEGIS1|<nonce_client>|<nonce_serveur>")
```

Règles appliquées par le moteur :

- la clé partagée fait au moins 16 octets (AEGIS en génère une de 32 octets) ;
- `hello` doit avoir `seq = 0` et `sid` vide ;
- l'horodatage du `hello` doit être à moins de `max_skew_ms` (30 s par défaut) de l'horloge
  du moteur ;
- un nonce déjà vu est refusé (un `hello` capturé ne peut pas être rejoué) ;
- le `hello` doit arriver dans les 5 secondes qui suivent la connexion ;
- en cas de refus, la connexion est fermée **sans explication** au pair ; la raison est
  consignée côté moteur.

Le `hello` annonce l'identité du terminal : `terminal` (`mt5` ou `simulator`), `login`,
`server`, `trade_mode` (`demo`, `contest`, `real`), `build`. Le moteur compare ensuite cette
identité à celle des messages `account` ; une incohérence bloque les entrées.

## 5. Protection contre le rejeu

Après la poignée de main, chaque trame reçue doit satisfaire, dans cet ordre :

1. HMAC valide avec la clé de session ;
2. version `1` et contenu valide (§ 7) ;
3. `sid` égal à celui de la session ;
4. `seq` égal au dernier numéro reçu **+ 1** exactement ;
5. `ts` à moins de `max_skew_ms` de l'horloge locale.

Une seule violation **ferme la session**. Conséquences :

- une trame rejouée, dupliquée ou arrivée dans le désordre est refusée (règle 4) ;
- une trame d'une session précédente est refusée (règles 1 et 3 : la clé de session change à
  chaque connexion) ;
- une trame modifiée est refusée (règle 1).

## 6. Messages

### Dans les deux sens

| `t` | Champs | Rôle |
|---|---|---|
| `heartbeat` | — | Signal de présence, toutes les 2 s dans chaque sens. |
| `bye` | `reason` | Fermeture volontaire. |
| `error` | `code`, `msg` | Refus d'une trame, juste avant la fermeture. |

### Connecteur → moteur

| `t` | Champs | Rôle |
|---|---|---|
| `hello` | `nonce`, `terminal`, `login`, `server`, `trade_mode`, `build` | Ouverture. |
| `spec` | `symbol`, `digits`, `point`, `contract_size`, `tick_size`, `tick_value`, `volume_min`, `volume_max`, `volume_step`, `stops_level`, `swap_long`, `swap_short`, `trade_mode` (`full`, `closeonly`, `disabled`), `currency_profit` | Spécification broker d'un instrument. |
| `tick` | `symbol`, `time`, `bid`, `ask` | Prix. |
| `account` | `login`, `server`, `currency`, `balance`, `equity`, `margin`, `margin_free`, `leverage`, `trade_mode`, `trade_allowed` | État du compte, chaque seconde. |
| `pos` | `snap`, `ticket`, `symbol`, `side`, `volume`, `price_open`, `sl`, `tp`, `profit`, `swap`, `commission`, `magic`, `comment`, `open_time` | Une position ouverte, dans un instantané. |
| `ord` | `snap`, `ticket`, `symbol`, `side`, `kind`, `volume`, `price`, `sl`, `tp`, `magic`, `comment`, `setup_time` | Un ordre en attente, dans un instantané. |
| `snap_end` | `snap`, `positions`, `orders` | Fin d'un instantané, avec le nombre d'éléments envoyés. |
| `deal` | `deal`, `position`, `symbol`, `side`, `volume`, `price`, `profit`, `swap`, `commission`, `time`, `comment`, `origin` | Clôture (totale ou partielle) constatée chez le broker. |
| `balance_op` | `deal`, `amount`, `time` | Dépôt, retrait ou crédit. |
| `result` | `cid`, `ok`, `retcode`, `ticket`, `price`, `volume`, `msg` | Réponse du broker à une instruction `order`. |
| `bar` | `symbol`, `tf`, `time`, `open`, `high`, `low`, `close`, `volume`, `spread`, `last` | Bougie historique, en réponse à `history_req`. |

`side` vaut `buy` ou `sell`. `kind` vaut `market`, `limit` ou `stop`. `tf` vaut `M1`, `M5`,
`M15`, `M30`, `H1`, `H4` ou `D1`. Les heures sont en millisecondes UTC.

**Instantané des positions.** Le connecteur envoie toutes les positions (`pos`) et tous les
ordres en attente (`ord`) avec le même numéro `snap`, puis `snap_end` avec leurs nombres. Le
moteur n'applique l'instantané que si les nombres correspondent. Il est envoyé toutes les
2 secondes et après chaque événement de trading.

**`deal.origin`** est le commentaire de l'ordre qui a ouvert la position, c'est-à-dire
l'identifiant AEGIS (`cid`) quand l'ordre vient d'AEGIS. C'est ce qui permet de prouver qu'un
ordre a été exécuté même si la position a été ouverte et clôturée avant toute confirmation.

### Moteur → connecteur

| `t` | Champs | Rôle |
|---|---|---|
| `welcome` | `nonce` | Réponse au `hello`. |
| `order` | `cid`, `auth_id`, `action`, `symbol`, `side`, `kind`, `volume`, `price`, `sl`, `tp`, `deviation`, `ticket`, `expires`, `magic` | Instruction de trading. |
| `history_req` | `symbol`, `tf`, `count` | Demande de bougies (le connecteur en renvoie 5 000 au plus). |

Un terminal qui envoie `order`, `history_req`, `welcome` ou un second `hello` voit sa session
fermée : ces messages n'existent que dans le sens moteur → connecteur.

## 7. Validation du contenu

Chaque trame décodée est contrôlée avant d'atteindre le moteur :

- nombres finis ; prix, volumes et tailles strictement positifs là où c'est exigé ;
- `ask ≥ bid` ; `high ≥ low` ;
- symboles : 32 caractères au plus, lettres, chiffres et `. _ - # + ! / & $` et espace ;
- textes bornés (64 à 512 caractères selon le champ), sans caractère de contrôle ;
- unité de temps connue ; `count` entre 1 et 100 000 ;
- identifiants (`login`, `ticket`, `deal`) strictement positifs.

Une trame qui échoue à ces contrôles ferme la session.

## 8. L'instruction `order`

| Champ | Sens |
|---|---|
| `cid` | Identifiant unique de l'instruction. Devient le commentaire de l'ordre dans MT5. |
| `auth_id` | Autorisation du moteur de risque qui a permis l'instruction (traçabilité). |
| `action` | `open` (entrée, au marché ou en attente), `modify` (SL/TP), `close` (totale ou partielle), `cancel` (ordre en attente). |
| `price` | Prix pour lequel le risque a été calculé. |
| `deviation` | Écart maximal accepté, en points, entre `price` et le prix du marché. |
| `ticket` | Position ou ordre visé (`modify`, `close`, `cancel`). |
| `expires` | Heure limite d'exécution (ms UTC, horloge du moteur). |
| `magic` | Numéro magique ; doit être celui du connecteur. |

Le moteur ne construit une instruction `open` qu'à partir d'une autorisation valide du moteur
de risque (usage unique, 3 s de validité par défaut) et **jamais en mode MONITOR**. Une
instruction `open` porte toujours un stop-loss et un take-profit.

Le connecteur revérifie, dans cet ordre, avant d'appeler le broker :

1. **doublon** : un `cid` déjà traité reçoit de nouveau la même réponse, sans nouvelle
   exécution ;
2. **expiration** : `expires` dépassé ⇒ refus (`retcode` 90001) ;
3. **numéro magique** différent ⇒ refus (90003) ;
4. pour `open` : compte réel sans l'option `InpAllowRealAccount` ⇒ refus ; ordre portant déjà
   ce `cid` sur le compte ⇒ refus ; SL ou TP absent ⇒ refus ; volume illégal pour
   l'instrument ⇒ refus (jamais corrigé) ; prix du marché à plus de `deviation` points de
   `price` ⇒ refus ;
5. pour `close`, `modify`, `cancel` : position ou ordre introuvable ⇒ refus (90002).

Codes propres à AEGIS : `90001` expirée, `90002` introuvable, `90003` refusée par le
connecteur. Les autres valeurs de `retcode` sont celles du broker (`TRADE_RETCODE_*`).

## 9. Accusé d'exécution

`result` rapporte **ce que le broker a répondu**, rien d'autre : `ok`, son code, le ticket, le
prix et le volume réellement exécutés, son message.

Le moteur ne considère jamais un ordre exécuté parce qu'il a été envoyé :

- `ok = true` avec un ticket ⇒ exécuté (ou partiel si le volume est inférieur, ou placé pour
  un ordre en attente) ;
- `ok = true` sans ticket ⇒ **pas** considéré comme exécuté ;
- `ok = false` ⇒ rejeté, avec le code et le message du broker ;
- aucune réponse dans le délai `result_timeout_ms` (5 s) ⇒ état `unknown`, tranché par la
  réconciliation avec les instantanés et les `deal` : position trouvée, clôture trouvée, ou
  absence prouvée.

## 10. Présence, délais, reconnexion

| Événement | Réaction |
|---|---|
| Aucun `hello` 5 s après la connexion | Connexion fermée. |
| Aucune trame du terminal pendant `peer_timeout_ms` (10 s) | Session fermée, entrées bloquées, état « déconnecté » affiché. |
| Aucune trame du moteur pendant 10 s | Le connecteur ferme et se reconnecte. |
| Connexion perdue | Le connecteur réessaie toutes les 3 s. Nouvelle poignée de main, nouvelle clé de session. |
| Reconnexion | Le connecteur renvoie spécifications, compte et instantané ; le moteur redemande l'historique et réconcilie les ordres en cours. |

Les instructions en file d'attente pendant une déconnexion sont **jetées** à la reconnexion :
elles sont périmées.

## 11. Erreurs

Sur une trame refusée, le moteur envoie `error` avec un code générique (`protocol` ou
`unexpected`) puis ferme. Le détail (HMAC invalide, séquence, horodatage, contenu) est écrit
dans le journal du moteur et dans le journal d'audit, pas envoyé au pair.

## 12. Clé partagée

- Lue dans la variable d'environnement `AEGIS_SHARED_KEY` ; à défaut, AEGIS crée
  `data/shared.key` (256 bits aléatoires, droits `0600`).
- Elle se saisit dans le paramètre `InpSharedKey` du connecteur.
- Elle n'est jamais écrite dans la configuration ni dans les journaux.

## 13. Ce que les tests vérifient

`crates/aegis-protocol/tests/protocol.rs`, `tests/mql5_compat.rs` et les tests unitaires de
`io` : aller-retour de tous les messages, trame non authentifiée ou modifiée, rejeu, doublon,
désordre, trame d'une autre session, horodatage hors fenêtre, mauvaise version, trame trop
grande, contenu invalide, mauvaise clé, `hello` rejoué, `welcome` forgé, vecteurs RFC 4231,
lecture interrompue sans perte d'octets, et acceptation des messages tels que le code MQL5
les fabrique. Détail dans `TEST_REPORT.md`.
