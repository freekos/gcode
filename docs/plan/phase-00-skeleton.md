# Фаза 0 — Скелет ✅

**Зачем:** точка отсчёта — репозиторий, в котором всё собирается, тестируется и публикуется с первого дня.

**Что получили:**
- Публичный репо [freekos/gcode](https://github.com/freekos/gcode) (MIT).
- Cargo workspace: `crates/gcode-core` (движок-библиотека) + `crates/gcode-cli` (бинарь `gcode` поверх ядра — для скриптов и будущего телеграм-бота).
- CI (GitHub Actions): форматирование + clippy + тесты на каждый пуш. Зелёный.
- `DESIGN.md` — зафиксированные продуктовые решения.

**Шаги:**
- [x] cargo workspace из двух крейтов
- [x] `gcode --version` работает
- [x] CI зелёный
- [x] README (манифест) + LICENSE + DESIGN.md
