# Ручная настройка интерфейса AMRI

UI отделён от VPN-ядра: изменение темы, расположения или графики не должно затрагивать routing,
secrets, transports и protection gate.

## Где менять

| Что | Windows | Android |
|---|---|---|
| Цвета, радиусы, основные отступы и размеры | `apps/windows/src/theme.rs` | `apps/android/app/src/main/java/ru/amri/vpn/AmriTheme.kt` |
| Расположение экранов и карточек | `apps/windows/src/main.rs` | `apps/android/app/src/main/java/ru/amri/vpn/MainActivity.kt` |
| Тексты и 9 языков | `crates/amri-core/src/i18n.rs` | `apps/android/app/src/main/res/values*/strings.xml` |
| Фон, иконка, кнопки | `assets/brand/` | тот же `assets/brand/`, Gradle копирует нужные файлы при сборке |

## Замена изображения или кнопки

1. Заменить файл с тем же именем в `assets/brand/`. Не создавать платформенную копию в `apps/`.
2. Проверить SVG/PNG визуально и убедиться, что прозрачный фон действительно прозрачный.
3. Получить новый Git blob ID: `git hash-object assets/brand/<имя-файла>`.
4. После осознанного утверждения изображения заменить соответствующий ID в
   `assets/brand/asset-blobs.lock`.
5. Запустить `python scripts/verify_visual_assets.py`, затем обычные platform tests/build.

Lock нужен не для запрета редактирования, а чтобы случайная или устаревшая картинка не попала в
сборку незаметно. `refresh-button.svg` станет доступна UI только вместе с реальным backend обновления
подписок.

## Правила безопасного изменения layout

- Не связывать visual state кнопки с обещанием реальной защиты: статус ON разрешает только общий
  public-tunnel gate.
- Сохранять accessibility description у icon buttons и адаптацию к длинным переводам/RTL.
- Не размещать credentials, subscription URL или raw destination в UI diagnostics.
- После изменения размеров проверить Windows minimum window и Android narrow/large screens.
