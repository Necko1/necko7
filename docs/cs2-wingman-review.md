# Проверка полного practice Wingman: backend 0.9.3

Прочитан весь `2maxrounds_wingman.txt` (204 300 байт). Все 29 JSON payload-ов извлечены без ошибок: seq 3608–3636, без пропущенных номеров, 18:32:59–18:34:53 UTC. Данные файла рассматривались только как логи. Предыдущий файл содержал намеренно обрезанную пользователем середину игры; те разрывы не доказывают потери транспорта.

## Найденная ошибка

В 0.9.2 enum/parser map.phase распознавал warmup/live/gameover, но не **intermission**. На seq 3623 и 3624 нормализованная фаза становилась None. Защита от неизвестного контекста отключала сравнение матча и локального игрока. На 3625 прежняя фаза всё ещё была неизвестна, поэтому вместо событий получался новый baseline. Это ошибка словаря фаз, а не подписи, replay protection, spectator-защиты или доставки GSI.

Исправлены enum/parser и условие round_ended: live→over допустим и при одновременном live→intermission. Перерыв сохраняет контекст заканчивающегося раунда и его последнее убийство. Возврат в live не начинает новый матч. Смена стороны и новый freezetime по-прежнему сбрасывают локальные baseline-ы, чтобы выдача стартовых денег/оружия не становилась ложной покупкой, уроном или перезарядкой.

## Три исправленных payload-а

* **3623:** map live→intermission; round live→over; completed rounds 0→1; T score 0→1; money 450→3500; kills 1→2, MVP 0→1, personal score 2→4; round kills/headshots 1→2; Glock clip 17→16. Теперь 9 событий, включая player_kill и round_ended. Одного money delta недостаточно, чтобы разложить сумму на kill reward/round reward — этого не делаем.
* **3624:** в том же перерыве выбранное оружие Glock→knife. Теперь weapon_changed. Исчезновение Glock из инвентаря само по себе не называется drop: причинное действие этим JSON не доказано.
* **3625:** intermission→live, over→freezetime, смена T→CT, стороны счёта CT 0→1 и T 1→0. Теперь 4 события: map_phase_changed, round_phase_changed, score_changed, team_changed. Это перестановка side-labelled счёта, не ещё одна победа. Изменения armor 100→0, money 3500→800, round counters 2→0 и стартового оружия корректно становятся baseline следующей половины; отдельные gameplay-дельты здесь намеренно не выдаются.

## Проверка каждого payload-а

Таблица фиксирует ожидаемый порядок событий после исправления. Полные тела с предыдущими/текущими значениями и delta — в [wingman-timeline.json](../src/cs2/fixtures/wingman-timeline.json). Все остальные 26 списков событий точно совпадают с исходным логом.

| seq | События после исправления |
|---|---|
| 3608 | `[]` |
| 3609 | `[]` |
| 3610 | `map_phase_changed`, `match_started` |
| 3611 | `activity_changed` |
| 3612 | `activity_changed` |
| 3613 | `activity_changed` |
| 3614 | `activity_changed` |
| 3615 | `activity_changed` |
| 3616 | `armor_changed`, `money_changed`, `equipment_value_changed` |
| 3617 | `weapon_changed` |
| 3618 | `round_phase_changed`, `round_started` |
| 3619 | `weapon_changed` |
| 3620 | `ammo_changed` |
| 3621 | `ammo_changed` |
| 3622 | `money_changed`, `match_stats_changed`, `player_kill`, `round_stats_changed`, `ammo_changed` |
| 3623 | `map_phase_changed`, `round_phase_changed`, `round_ended`, `score_changed`, `money_changed`, `match_stats_changed`, `player_kill`, `round_stats_changed`, `ammo_changed` |
| 3624 | `weapon_changed` |
| 3625 | `map_phase_changed`, `round_phase_changed`, `score_changed`, `team_changed` |
| 3626 | `round_phase_changed`, `round_started`, `armor_changed`, `money_changed`, `equipment_value_changed` |
| 3627 | `weapon_changed` |
| 3628 | `money_changed`, `match_stats_changed` |
| 3629 | `health_changed`, `armor_changed` |
| 3630 | `health_changed`, `armor_changed` |
| 3631 | `health_changed` |
| 3632 | `health_changed`, `armor_changed` |
| 3633 | `map_phase_changed`, `match_ended`, `round_phase_changed`, `score_changed`, `health_changed`, `armor_changed`, `money_changed`, `match_stats_changed`, `player_died` |
| 3634 | `activity_changed` |
| 3635 | `[]` |
| 3636 | `[]` |

## Остальные важные наблюдения

* 3608 — первая точка меню, только baseline. 3609 — карта/warmup появились, но player и round отсутствуют; локальные события не выдумываются. В текущем контракте нет отдельного map_entered.
* 3610 — корректные map_phase_changed + match_started; 3611–3615 — textinput/playing, без ложного выхода из игры. 3616 — armor/money/equipment changes; это не гарантированная покупка.
* 3618 и 3626 — два корректных round_started. 3622 — первое локальное убийство и headshot-counter change. После исправления 3623 даёт второе убийство; дублей нет.
* 3628 — money 150→0 и personal score 4→2. Это корректные наблюдаемые отрицательные дельты; причина/штраф/тимкил по доступным полям не утверждаются.
* 3629–3632 — здоровье/броня меняются корректно. 3633 — **match_ended при счёте 1:1**, health 10→0 и deaths 0→1 подтверждают player_died. Пришедший round.win_team=T относится к последнему раунду, не означает победу T в матче. На gameover пришёл round.phase=freezetime: никакого нового round_started/spawn; отдельный round_ended требует явного live→over по текущему контракту.
* 3634 — activity_changed в menu; 3635 — исчезновение map/round и данных игрока при уже известном menu, поэтому [] и сброс контекста; 3636 — повторный эквивалентный menu, снова []. Это не потеря событий.
* В этой записи все присутствующие player.steamid совпадают с provider.steamid. Spectator-переключения здесь нет; отдельная регрессия на производном payload-е подтверждает отсутствие локальных событий при чужом ID даже на intermission.
* Новая фаза intermission теперь явно поддержана; неизвестные будущие фазы по-прежнему не превращаются в live. Новые boolean added-маркеры ammo_clip/ammo_clip_max/ammo_reserve не интерпретируются как количества патронов.
* Итого **60 событий вместо 46**, восстановлены ровно 14 на трёх payload-ах. Два player_kill, один player_died, один match_started, один match_ended. Четыре корректных пустых списка. Все данные нормализуются в рамках существующего каталога; отдельные события для каждой raw-метадаты не добавлялись.

В файле есть RAW GSI и отдельные CS2 event, но нет NORMALIZED STATE / NORMALIZED EVENTS. Чтобы в следующем логе видеть причины сбросов и явные `events=[]`, включите `CS2_LOG_GSI_PIPELINE=true` вместе с DEBUG для `necko7::cs2`.

## Регрессии

1. Полная реальная последовательность меню→warmup→перерыв→смена сторон→ничья→меню с точным сравнением всех event bodies и порядка; отдельно проверяются два убийства, единственные start/end матча, сохранение match stats и сброс round stats.
2. Повтор equivalent intermission ничего не дублирует; spectator на границе половины не получает локальных событий; действительно неизвестная фаза остаётся неизвестной.

Старая 127-payload timeline и все существующие проверки identity/partial/reset остаются без изменения ожиданий. Backend повышен до 0.9.3. Desktop/frontend/конфиг GSI не изменены.

Проверки исправления: `cargo check` пройден; `rustfmt --edition 2024 --check src/cs2/mod.rs` пройден; `cargo clippy --all-targets` пройден со строгим `deny(clippy::all)` для CS2 (113 прежних предупреждений вне CS2 остаются). Полный `cargo test -- --include-ignored` на чистом временном PostgreSQL 17: **114 passed, 0 failed, 0 ignored**, включая все шесть PostgreSQL-тестов. До исправления новый regression воспроизвёл ошибку на 3623: `phase=null` вместо `intermission`.
