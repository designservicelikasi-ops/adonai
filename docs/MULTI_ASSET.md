# Multi-actifs, exposition globale et actualités (0.3)

## Instruments

Chaque instrument a **sa propre spécification broker** (taille de contrat, tick, valeur du
tick, volume minimum et pas, stops level, freeze level, swaps, devises) reçue du terminal ;
rien n'est supposé identique d'un symbole à l'autre. Catégorie déduite du nom (forex, métaux,
indices, énergie, crypto ; « autre » sinon). Les instruments disponibles sont ceux que le
terminal publie (CRYPTO seulement si le broker et le connecteur les proposent).

`set_symbol_enabled` active ou désactive les **nouvelles entrées** d'un instrument (contrôle
`instrument_actif`) ; les positions ouvertes restent gérées et protégées. Le choix est conservé
après redémarrage.

## Global Exposure Manager

- Corrélations des rendements entre chaque paire d'instruments, sur 300 bougies H1 (sinon
  M15), recalculées toutes les 15 minutes ; au moins 50 rendements communs.
- Contrôle `exposition_correlee` : les positions sur des instruments corrélés (|ρ| ≥
  `correlation_threshold`, 0,7) dans la **même direction effective** (même sens si ρ > 0, sens
  opposé si ρ < 0) sont un seul pari ; leur risque aux stops + celui de la nouvelle entrée
  est limité à `max_correlated_risk_pct` (par défaut 2 x le risque par transaction).
- S'ajoutent : `exposition` (notionnel total), `exposition_devise` (par devise, sans
  compensation), `risque_portefeuille`, `marge_libre`.

## Actualités

Le module News (calendrier public, D30) s'applique à chaque instrument par ses devises (base
et profit : XAUUSD, NAS100, USOIL sont concernés par les annonces USD). Chaque instrument
affiche sa recommandation (autorisé / prudence / suspendu) et la raison. Sans calendrier
joignable : prudence (jamais « calme » par défaut). Aucun modèle de langage n'est utilisé
(D30) : un résumé par IA, s'il était ajouté, ne serait jamais une autorité d'exécution.
