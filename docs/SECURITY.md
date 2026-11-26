# Sécurité

## Surfaces exposées

| Liaison | Protection | Recommandation |
|---|---|---|
| Connecteur MT5 → moteur (TCP, port 7878) | HMAC-SHA256 sur chaque trame, clé de session dérivée de deux nonces, numéro de séquence strict, fenêtre d'horodatage, lecture bornée. **Pas de chiffrement.** | Laisser sur la boucle locale (MT5 et moteur sur le même VPS Windows) ou dans un tunnel (WireGuard, SSH). Jamais exposé sur Internet. |
| AEGIS Desktop → moteur (HTTPS, port 7879) | TLS 1.2/1.3 (rustls), certificat auto-signé **épinglé** par son empreinte SHA-256, jeton porteur de 48 caractères, limitation des échecs (10 par minute et par adresse → HTTP 429). | Sur un VPS : `api.listen = "0.0.0.0:7879"` et règle de pare-feu limitant l'accès à votre adresse. |
| Webhook d'alertes (sortant) | HTTPS uniquement (ou `http://127.0.0.1` vers un relais local). | Facultatif. |
| Telegram (sortant) | HTTPS vers `api.telegram.org`. Le jeton du bot est lu dans une variable d'environnement (`ops.telegram_token_env`) au moment de l'envoi : jamais dans la configuration, la base, les journaux ni les messages d'erreur. Le message contient le niveau, la source, le texte de l'alerte et le numéro du compte. | Facultatif (`ops.telegram_chat_id` vide = désactivé). |
| Calendrier économique (sortant) | HTTPS vers la source configurée. Aucune donnée personnelle envoyée. | Désactivable (`news.enabled = false`). |

## Secrets

| Secret | Origine | Stockage |
|---|---|---|
| Clé partagée MT5 | `AEGIS_SHARED_KEY` ou `data/shared.key` (256 bits aléatoires) | fichier en droits `0600` ; jamais dans la configuration ni les journaux |
| Jeton de l'API | `AEGIS_API_TOKEN` ou `data/api.token` (192 bits aléatoires) | idem |
| Clé privée TLS | `data/tls/key.pem` (ECDSA P-256) | idem |
| Profil de connexion du desktop | dossier de configuration de l'application (`connection.json`) | droits `0600` sous macOS/Linux ; sous Windows, protégé par le profil utilisateur |

`aegisd pairing` affiche l'adresse, l'empreinte et le jeton à saisir dans AEGIS Desktop. Le
desktop refuse toute adresse `http://` qui n'est pas la boucle locale, et refuse un certificat
dont l'empreinte diffère de celle appairée (interception ou certificat régénéré).

## Ce que l'API ne permet pas

- arrêter le moteur (c'est le rôle du gestionnaire de service) ;
- lire ou écrire un fichier hors de `<data_dir>/files/` (chemins absolus et `..` refusés) ;
- contourner le moteur de risque : l'API envoie les mêmes commandes que l'interface, validées
  par le moteur.

## Autorisation des ordres

- Un ordre n'existe qu'à partir d'une autorisation signée par le moteur de risque, valable
  3 secondes, à usage unique, liée à une décision enregistrée en base **avant** tout envoi.
- Le connecteur revérifie : authentification, expiration, doublon, volume, écart de prix.
- Un état inconnu n'est jamais interprété comme une absence d'exécution : réconciliation avec
  les positions et transactions du broker, qui fait foi.
- Le stop-loss est obligatoire et ne peut être déplacé que dans le sens protecteur.

## Journal d'audit

Ajout seul, chaîné par SHA-256 (chaque entrée contient l'empreinte de la précédente).
`aegisd verify-audit` ou le bouton « Vérifier l'audit » détecte toute modification. Y sont
écrits : démarrages, changements de mode, arrêt d'urgence, modifications de configuration,
décisions de gestion des positions, transactions, ordres, promotions, entraînements de modèles.

## Limites connues

- La liaison MT5 n'est pas chiffrée (MQL5 ne permet pas de vérifier un certificat épinglé ;
  voir `DECISIONS.md`, D23).
- Le profil du desktop n'utilise pas le trousseau du système d'exploitation.
- Le code du service Windows et le connecteur MQL5 n'ont pas encore été compilés sur Windows
  (voir `PROJECT_STATUS.md`).
