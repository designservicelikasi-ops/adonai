# Rapport de validation — lr-EURUSD.sim-M15-h4-1794773737504

- Données : EURUSD.sim M15, origine **synthetic**, 29896 échantillons
- Cible : P(close[t+4] > close[t])
- Méthode : walk-forward, 5 folds, expanding window, embargo 4 bars, Platt calibration on the last 20 % of each training window

| Pli | Échantillons | AUC | Brier | Log-loss |
|---|---|---|---|---|
| 1 | 4982 | 0.5983 | 0.2435 | 0.6803 |
| 2 | 4982 | 0.6244 | 0.2400 | 0.6729 |
| 3 | 4982 | 0.5914 | 0.2432 | 0.6796 |
| 4 | 4982 | 0.5960 | 0.2431 | 0.6792 |
| 5 | 4986 | 0.5980 | 0.2423 | 0.6775 |

- AUC moyenne hors échantillon : **0.6016** (0,50 = hasard)
- Brier moyen : 0.2424 — référence naïve : 0.2500 — amélioration : +0.0076
- Erreur de calibration (ECE) : 0.0071
- Taux de hausses : 0.492

Ces chiffres décrivent des données passées. Ils ne garantissent aucune précision future.
Le verdict d'utilisation est rendu par AEGIS (`aegisd verify-model`), pas par ce script.

**Modèle entraîné sur des données SYNTHÉTIQUES : inutilisable pour l'exécution, quel que soit son score.**
