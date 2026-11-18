# Installation sous Windows

## AEGIS Desktop (poste de supervision)

1. Récupérer dans `RELEASES/windows/` l'installateur `AEGIS_<version>_x64-setup.exe`
   (ou `AEGIS_<version>_x64_fr-FR.msi` pour un déploiement géré).
2. Lancer l'installateur. Installation **par utilisateur** (pas de droits administrateur)
   dans `%LOCALAPPDATA%\AEGIS`. L'installateur contient l'application **et** le moteur
   `aegisd.exe`, placé à côté de l'application.
3. Ouvrir AEGIS. Page *Connexion* :
   - **Moteur local** : « Démarrer le moteur local » lance `aegisd.exe` en processus détaché
     (données dans `%APPDATA%\app.aegis.desktop\engine\`), s'y connecte et mémorise la
     connexion. Fermer la fenêtre ne l'arrête pas.
   - **Moteur distant** : adresse `https://<VPS>:7879`, jeton et empreinte donnés par
     `aegisd pairing` sur le serveur (voir `VPS_DEPLOYMENT.md`).
4. L'application démarre et reste utilisable sans MetaTrader 5 : l'état « MT5 déconnecté » est
   affiché et aucune fonction de trading n'est active (mode MONITOR).

Prérequis : Windows 10 (1803+) ou 11 x64, WebView2 (présent sur Windows 11 ; l'installateur
le télécharge sinon).

### Désinstallation

*Paramètres → Applications → AEGIS → Désinstaller* (ou `uninstall.exe` dans le dossier
d'installation). Les données du moteur local (`%APPDATA%\app.aegis.desktop\`) sont conservées :
supprimez ce dossier pour tout effacer. Un moteur local encore en marche s'arrête depuis le
Gestionnaire des tâches (`aegisd.exe`) — il s'arrête proprement et repart en MONITOR au
prochain démarrage.

## Moteur en service Windows (VPS ou poste dédié)

Voir `VPS_DEPLOYMENT.md`, § 1 : `packaging\windows\install-service.ps1` (PowerShell
administrateur) installe le service `AegisEngine` avec redémarrage automatique.

## Connecteur MetaTrader 5

Voir `MT5_CONNECTOR.md`.

## État de vérification

Les installateurs Windows sont produits par le pipeline CI sur un runner Windows natif
(`.github/workflows/build.yml`, job `desktop`), qui installe aussi l'application en mode
silencieux, vérifie la présence de `aegisd.exe` et la désinstalle. Tant qu'un tel run n'a pas
été observé, ces étapes sont **NON VÉRIFIÉES** (voir `PROJECT_STATUS.md`).
