# Configuration

Deux niveaux :

1. **Le fichier du serveur** (`config/aegis.toml`, modèle commenté : `config/aegis.example.toml`)
   — adresses, sécurité, modules, sauvegardes. Lu au démarrage. Une clé inconnue ou une valeur
   hors bornes **empêche le démarrage** : rien n'est ignoré ni corrigé en silence.
   `aegisd --config FICHIER print-config` affiche la configuration effective.
2. **La configuration de risque**, modifiable depuis AEGIS Desktop (page *Risque &
   configuration*) ou l'API (`set_risk_config`). Chaque changement est validé, enregistré en
   base avec un numéro de version et écrit au journal d'audit. Dès qu'une version existe en
   base, elle prend le pas sur la section `[risk]` du fichier.

Les paramètres des stratégies se modifient de la même façon (page *Stratégies*, commande
`set_strategy_config`) ; une stratégie modifiée revient au stade Recherche.

## Fichier du serveur

| Section | Rôle | Clés principales |
|---|---|---|
| `[engine]` | données, instruments, unités | `data_dir`, `symbols` (vide = ceux du terminal), `signal_timeframes` (M1, M5, M15, M30, H1), `history_bars` |
| `[bridge]` | liaison avec le connecteur MT5 | `listen` (laisser `127.0.0.1:7878`), `max_skew_ms`, `peer_timeout_ms`, `shared_key_env` |
| `[api]` | API de contrôle (desktop) | `enabled`, `listen`, `token_env`, `tls` (obligatoire hors boucle locale), `tls_names` |
| `[risk]` | valeurs **initiales** de risque | voir ci-dessous |
| `[market]` | moteur de marché | `timeframes`, `stale_after_ms`, `abnormal_spread_factor`, `max_clock_offset_ms` |
| `[execution]` | ordres | `magic` (identique dans le connecteur), `result_timeout_ms`, `reconcile_grace_ms` |
| `[learning]` | modèle logistique Python (filtre) | `model_path`, `[learning.gate]` |
| `[probability]` | P(TP avant SL) | grilles TP/SL en ATR, horizon, walk-forward, critères de calibration, scénario affiché |
| `[news]` | calendrier économique | `enabled`, `sources`, fenêtres autour des annonces, marge de prudence |
| `[shadow]` | laboratoire Shadow | `enabled`, `variants`, `forward_min_trades` |
| `[ops]` | sans surveillance | `heartbeat_file`, `backup_interval_hours`, `backup_keep`, `alert_webhook_url`, `telegram_chat_id`, `telegram_token_env`, `disconnect_alert_s` |
| `[safety]` | réel | `live_trading_enabled` (faux par défaut) |
| `[retention]` | purge | `candle_days`, `snapshot_days` |

## Configuration de risque

| Clé | Défaut | Bornes | Effet |
|---|---|---|---|
| `daily_goal.enabled` | `true` | | Daily Profit Goal Manager |
| `daily_goal.amount` | 15 | > 0 | objectif du jour en devise du compte |
| `daily_goal.basis` | `net` | `net`, `gross` | net = après commission, swap, frais ; brut = avant |
| `daily_goal.policy` | `stop_entries` | `stop_entries`, `protect_gains`, `track_only` | ce qui se passe une fois atteint |
| `daily_goal.protect_fraction` | 0,8 | 0,5–1 | `protect_gains` : part du meilleur gain à conserver |
| `buy_min_probability` / `sell_min_probability` | 0,75 | 0,50–0,99 | P(TP avant SL) minimale ; jamais abaissée automatiquement |
| `min_net_expected_profit` | 0,50 | ≥ 0 | espérance nette minimale par transaction |
| `risk_per_trade_pct` | 0,5 | 0,01–5 | perte si le stop est touché, en % de l'équité |
| `max_daily_loss_pct` | 2 | 0,1–20 | résultat net du jour (flottant compris) qui bloque les entrées |
| `max_drawdown_pct` | 10 | 0,5–50 | déclenche l'arrêt d'urgence |
| `max_lots_per_trade` | 0 (off) | 0–1000 | plafond de taille par transaction (micro-trading) ; ne peut que réduire la taille calculée au risque |
| `max_open_positions`, `max_positions_per_symbol` | 3, 1 | 1–100 | |
| `max_notional_leverage`, `max_currency_exposure` | 5, 5 | 0,1–100 | exposition totale / par devise, en multiple de l'équité |
| `max_spread_points`, `max_slippage_points` | 25, 10 | | |
| `max_trades_per_day` | 500 | 1–10 000 | 2 000 est une valeur valide |
| `max_orders_per_minute` | 60 | 1–600 | |
| `max_consecutive_losses`, `loss_cooldown_minutes` | 5, 30 | 0–100, 0–1440 | pause après une série de pertes |
| `max_cost_to_gross_ratio`, `cost_check_min_trades` | 0,6, 20 | 0–1 | bloque quand les coûts mangent les micro-gains |
| `trading_windows` | `[]` | `HH:MM` UTC | plages autorisées ; vide = toujours |
| `commission_per_lot_round_turn` | 7 | ≥ 0 | **à régler selon votre broker** (MT5 ne la fournit pas) |
| `expected_slippage_points`, `conversion_fee_pct`, `expected_holding_nights` | 2, 0, 0 | | estimation des coûts |
| `position.*` | voir l'exemple | | Position Intelligence : SECURE, stop suiveur, rapprochement du TP, sortie au temps ou au net |
| `kill_cancel_pending`, `kill_position_policy` | `true`, `keep` | | arrêt d'urgence |
| `max_tick_age_ms`, `max_account_age_ms`, `authorization_ttl_ms` | 5000, 15000, 3000 | | fraîcheur |

## Exemples

Objectif de 15 USD **brut**, puis continuer en protégeant 80 % du meilleur gain :

```toml
[risk.daily_goal]
amount = 15.0
basis = "gross"
policy = "protect_gains"
protect_fraction = 0.8
```

Micro-trading à volume élevé (le moteur ne trade pas plus pour autant : chaque entrée reste
soumise à la probabilité, à l'espérance nette et aux coûts) :

```toml
[risk]
max_trades_per_day = 2000
max_orders_per_minute = 120
max_open_positions = 5
```
