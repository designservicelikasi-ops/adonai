# Déploiement (index)

Ce document de la version 0.1 est remplacé par :

| Sujet | Document |
|---|---|
| Poste Windows (application, moteur local, service) | `INSTALLATION_WINDOWS.md` |
| Poste ou serveur Linux | `INSTALLATION_LINUX.md` |
| VPS (service, pare-feu, appairage, fonctionnement continu) | `VPS_DEPLOYMENT.md` |
| Connecteur MT5 | `MT5_CONNECTOR.md` |
| Secrets, TLS, jetons | `SECURITY.md` |
| Compilation | `BUILD_GUIDE.md` |

## Liste de contrôle avant un compte de démonstration

- [ ] `AegisSelfTest` affiche « ALL PASSED » dans MT5
- [ ] le connecteur se connecte ; la barre d'état affiche « MT5 connecté », le bon compte, type DEMO
- [ ] la commission par lot est renseignée (*Risque & configuration → Coûts*)
- [ ] le plafond de lots et l'objectif quotidien sont réglés
- [ ] les spécifications des instruments sont cohérentes avec MT5 (*Système*, instruments)
- [ ] un modèle de probabilité a été entraîné sur l'historique **broker** (*Probabilités*)
- [ ] une transaction de démo passe : statut `filled`, position visible des deux côtés avec SL et TP, journal rempli
- [ ] la gestion de position (SECURE) est visible dans MT5 (stop déplacé)
- [ ] l'arrêt d'urgence bloque une entrée ; la réinitialisation fonctionne
- [ ] couper le réseau : « MT5 déconnecté », alerte ; reconnexion et réconciliation
- [ ] sauvegarde créée et restauration essayée (`BACKUP_RECOVERY.md`)
