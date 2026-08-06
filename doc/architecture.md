⏺ flingdlna - Обзор проекта

  Архитектура

  flingdlna/
  ├── Cargo.toml                    # Workspace манифест
  ├── src/
  │   ├── main.rs                   # CLI точка входа (clap)
  │   └── tui/                      # Интерактивный TUI (ratatui)
  │       ├── mod.rs                # Event loop, run_tui()
  │       ├── app.rs                # App state + methods
  │       ├── render.rs             # UI rendering
  │       ├── input.rs              # Keyboard handling
  │       ├── actions.rs            # Async command processing
  │       ├── types.rs              # Enums: Action, BackgroundResult
  │       └── colors.rs             # Theme colors
  └── crates/
      ├── dlna-core/                # Ядро: типы, SSDP, конфиги
      ├── dlna-server/              # DMS: Media Server + metadata (mp4/matroska)
      ├── dlna-controller/          # DMC: Controller + UPnP Events
      ├── dlna-combo/               # Unified API + Daemon + Queue
      └── flingdlna-ffi/            # C FFI для iOS/macOS приложений

  ---
  1. dlna-core — Ядро библиотеки

  Источник: Написано с нуля, с заимствованием идей из VuIO (src/ssdp.rs)

  Файлы:

  | Файл      | Назначение                                                                        |
  |-----------|-----------------------------------------------------------------------------------|
  | types.rs  | Основные структуры: Renderer, MediaFile, ServerInfo, TransportState, PlaybackInfo |
  | error.rs  | Enum Error с вариантами для IO, Network, SSDP, UPnP, XML ошибок                   |
  | config.rs | ServerConfig, ControllerConfig, ComboConfig                                       |
  | ssdp.rs   | Unified SSDP — единый модуль для discovery и announcement                         |

  ssdp.rs — Ключевой модуль

  pub struct SsdpService {
      local_ip: IpAddr,
      http_port: u16,
      server_info: Option<ServerInfo>,  // Для режима сервера
      // ...
  }

  Функции:
  - discover_renderers() — отправляет M-SEARCH, парсит ответы, получает device description XML
  - start_announcer() — запускает фоновую задачу для NOTIFY announcements
  - handle_msearch() — отвечает на входящие M-SEARCH запросы

  Откуда взято:
  - Логика announcement из VuIO src/ssdp.rs (NOTIFY пакеты, device types)
  - Discovery написан с нуля (VuIO не делал discovery)

  ---
  2. dlna-server — DLNA Media Server (DMS)

  Источник: Адаптировано из https://github.com/vuiodev/vuio

  Файлы и их происхождение:

  | Файл                 | Источник из VuIO    | Назначение                     |
  |----------------------|---------------------|--------------------------------|
  | http.rs              | src/web/handlers.rs | Axum роуты, Range requests     |
  | content_directory.rs | src/web/handlers.rs | SOAP Browse action             |
  | didl.rs              | src/web/xml.rs      | DIDL-Lite XML генерация        |
  | description.rs       | src/web/xml.rs      | Device/Service description XML |
  | scanner.rs           | src/media.rs        | Сканирование директорий        |
  | state.rs             | Новый               | In-memory хранилище файлов     |

  http.rs — HTTP сервер

  Роуты:
  GET  /description.xml           → Device description (UPnP)
  GET  /ContentDirectory/scpd.xml → Service description
  POST /ContentDirectory/control  → SOAP Browse action
  GET  /media/{id}                → Streaming с Range support

  Range requests (из VuIO parse_range_header):
  // Поддерживает:
  // bytes=0-499      → первые 500 байт
  // bytes=500-       → от 500 до конца
  // bytes=-100       → последние 100 байт

  content_directory.rs — ContentDirectory Service

  SOAP Actions:
  - Browse — основной метод для навигации по контенту
  - GetSystemUpdateID — версия контента
  - GetSearchCapabilities / GetSortCapabilities

  ObjectID иерархия:
  "0"     → Root (возвращает Video/Audio/Image контейнеры)
  "video" → Все видео файлы
  "audio" → Все аудио файлы
  "image" → Все изображения

  didl.rs — DIDL-Lite XML

  Из VuIO src/web/xml.rs:
  <DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/">
    <item id="abc123" parentID="video" restricted="1">
      <dc:title>movie.mp4</dc:title>
      <upnp:class>object.item.videoItem</upnp:class>
      <res protocolInfo="http-get:*:video/mp4:DLNA.ORG_OP=01">
        http://192.168.1.100:8080/media/abc123
      </res>
    </item>
  </DIDL-Lite>

  description.rs — UPnP Device Description

  Из VuIO src/web/xml.rs:
  <root xmlns="urn:schemas-upnp-org:device-1-0">
    <device>
      <deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>
      <friendlyName>flingdlna</friendlyName>
      <serviceList>
        <service>
          <serviceType>urn:schemas-upnp-org:service:ContentDirectory:1</serviceType>
          ...
        </service>
      </serviceList>
    </device>
  </root>

  ---
  3. dlna-controller — DLNA Controller (DMC)

  Источник: Написано с нуля (изначально планировали обёртку над crab-dlna, но из-за конфликта http версий реализовали сами)

  Файлы:

  | Файл         | Назначение                                      |
  |--------------|-------------------------------------------------|
  | discover.rs  | Получение device description по URL             |
  | transport.rs | AVTransport SOAP actions (play/pause/stop/seek) |
  | streaming.rs | Локальный HTTP сервер для push файлов + субтитры|
  | subtitles.rs | Поиск и генерация DIDL с субтитрами             |
  | eventing.rs  | UPnP event subscriptions (GENA)                 |
  | cache.rs     | Кэш устройств для быстрого доступа              |

  transport.rs — AVTransport Control

  SOAP Actions (реализованы через raw HTTP):

  // SetAVTransportURI — установить URL для воспроизведения
  pub async fn set_uri(renderer: &Renderer, uri: &str, mime_type: Option<&str>)

  // Play/Pause/Stop
  pub async fn play(renderer: &Renderer)
  pub async fn pause(renderer: &Renderer)
  pub async fn stop(renderer: &Renderer)

  // Seek — перемотка
  pub async fn seek(renderer: &Renderer, position: Duration)

  // GetPositionInfo — текущая позиция
  pub async fn get_position_info(renderer: &Renderer) -> PlaybackInfo

  // GetTransportInfo — состояние (Playing/Paused/Stopped)
  pub async fn get_transport_state(renderer: &Renderer) -> TransportState

  SOAP формат:
  <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
    <s:Body>
      <u:Play xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
        <InstanceID>0</InstanceID>
        <Speed>1</Speed>
      </u:Play>
    </s:Body>
  </s:Envelope>

  streaming.rs — Push Local Files

  Когда делаешь flingdlna push movie.mp4 --device "LG":
  1. Запускается локальный HTTP сервер на порту 9000
  2. Файл становится доступен по http://YOUR_IP:9000/media
  3. Если рядом есть .srt/.vtt/.ass файл — доступен по /subtitle
  4. URL отправляется на TV через SetAVTransportURI (с DIDL метаданными)
  5. TV стримит файл с твоего компьютера

  subtitles.rs — External Subtitles

  Автоматический поиск субтитров:
  - Ищет файл с тем же именем: movie.mp4 → movie.srt, movie.vtt, movie.ass
  - Поддерживаемые форматы: .srt, .vtt, .sub, .ass, .ssa

  Samsung-совместимый DIDL с субтитрами:
  <DIDL-Lite>
    <item>
      <res>http://192.168.1.100:9000/media</res>
      <sec:CaptionInfoEx sec:type="srt">
        http://192.168.1.100:9000/subtitle
      </sec:CaptionInfoEx>
      <sec:CaptionInfo sec:type="srt">
        http://192.168.1.100:9000/subtitle
      </sec:CaptionInfo>
    </item>
  </DIDL-Lite>

  ---
  4. dlna-combo — Unified API + Daemon

  Источник: Написано с нуля

  **Feature Flags:**
  | Feature     | Default | Описание                                   |
  |-------------|---------|-------------------------------------------|
  | `chromecast`| No      | Поддержка Google Cast устройств           |
  | `daemon`    | No      | Unix socket daemon (не нужен для FFI)     |

  Файлы:

  | Файл        | Назначение                                        | Feature   |
  |-------------|---------------------------------------------------|-----------|
  | daemon/     | Daemon process, Unix socket IPC, command handling | `daemon`  |
  | protocol.rs | JSON protocol: Command, Response, Notification    | always    |
  | queue.rs    | Queue management: add, remove, shuffle, repeat    | always    |
  | database.rs | SQLite: watch history, positions, device cache    | always    |
  | lib.rs      | DlnaCombo unified API                             | always    |

  Daemon IPC Protocol:
  - Unix socket: ~/Library/Application Support/FlingDLNA/flingdlna.sock (macOS)
  - Unix socket: ~/.local/share/flingdlna/flingdlna.sock (Linux)
  - JSON-based commands/responses
  - UPnP event subscriptions and notifications

  Queue Features:
  - Add files/URLs to queue
  - Shuffle mode (Fisher-Yates)
  - Repeat modes: none, one, all
  - Auto-advance to next track

  Объединяет server и controller в один интерфейс:

  pub struct DlnaCombo {
      server: Option<MediaServer>,
      controller: Controller,
  }

  impl DlnaCombo {
      // Server
      pub async fn start_server(&mut self)
      pub fn add_media_directory(&self, path: PathBuf)

      // Controller
      pub async fn discover_renderers(&self) -> Vec<Renderer>
      pub async fn push(&self, file: &Path, renderer: &Renderer)
      pub async fn play_url(&self, url: &str, renderer: &Renderer)
      pub async fn pause(&self, renderer: &Renderer)
      pub async fn stop(&self, renderer: &Renderer)
  }

  ---
  5. flingdlna-ffi — C FFI для iOS/macOS

  Источник: Написано с нуля

  Предоставляет C-совместимый интерфейс для встраивания в нативные приложения.
  Компилируется как статическая библиотека (staticlib).

  **Особенности сборки:**
  - НЕ включает `daemon` feature (Unix sockets не нужны)
  - НЕ включает CLI зависимости (clap, ratatui, crossterm)
  - Включает `chromecast` по умолчанию
  - Telegram-интеграция реализована в Swift-клиенте macOS и не входит в Rust-ядро или C FFI

  Файлы:

  | Файл   | Назначение                                    |
  |--------|-----------------------------------------------|
  | lib.rs | FFI функции, типы, глобальное состояние       |

  FFI Функции:
  - fling_init(serve_dir) — инициализация библиотеки
  - fling_shutdown() — завершение работы
  - fling_list_renderers(timeout) — поиск устройств
  - fling_list_media() — список медиафайлов
  - fling_get_status(device) — статус воспроизведения
  - fling_play/pause/stop/seek — управление воспроизведением
  - fling_get_volume/set_volume — громкость
  - fling_play_media/play_url — начать воспроизведение

  Сборка:
  cargo build -p flingdlna-ffi --release
  # Результат: target/release/libflingdlna_ffi.a

  Генерация заголовков:
  cbindgen --config cbindgen.toml --crate flingdlna-ffi --output include/flingdlna.h

  ---
  6. CLI и TUI (src/)

  Источник: Написано с нуля с использованием clap и ratatui

  Структура:
  src/
  ├── main.rs       # CLI точка входа, clap парсинг
  └── tui/          # Интерактивный TUI (рефакторинг)
      ├── mod.rs    # run_tui(), main event loop
      ├── app.rs    # App struct, state management
      ├── render.rs # UI рендеринг (header, progress, lists)
      ├── input.rs  # Обработка клавиш
      ├── actions.rs# Async actions (spawn + channel)
      ├── types.rs  # Action, BackgroundResult, Focus
      └── colors.rs # Color constants

  TUI Architecture:
  - Event Loop: poll(50ms) + try_recv() для non-blocking
  - Background Tasks: tokio::spawn() с mpsc channel
  - State Updates: через BackgroundResult enum
  - Progress Interpolation: плавный прогресс бар между daemon polls

  # Запуск сервера
  flingdlna serve --dir ~/Movies --port 8080 --name "My Mac"

  # Поиск устройств
  flingdlna list --timeout 5

  # Push файла
  flingdlna push movie.mp4 --device "LG"
  flingdlna push movie.mp4 --device "http://192.0.2.1:1652/"

  # Управление воспроизведением
  flingdlna pause --device "LG"
  flingdlna stop --device "LG"
  flingdlna seek 01:30:00 --device "LG"
  flingdlna status --device "LG"

  # TUI
  flingdlna tui

  ---
  Ключевые решения

  | Решение        | Было запланировано    | Что сделали                              |
  |----------------|-----------------------|------------------------------------------|
  | HTTP фреймворк | axum                  | axum 0.8                                 |
  | SSDP           | Unified socket        | Unified (discovery + announcement)       |
  | Storage        | In-memory             | Vec<MediaFile> в RwLock                  |
  | Controller     | Обёртка над crab-dlna | Полная реализация с нуля (raw HTTP/SOAP) |
  | rupnp          | Для AVTransport       | Не используем (конфликт http версий)     |

  ---
  Зависимости

  # Core
  tokio       # Async runtime
  axum        # HTTP framework
  url         # URL parsing

  # UPnP (только для device discovery)
  rupnp       # Используется только в dlna-core для парсинга device XML
  ssdp-client # Не используется напрямую (rupnp dependency)

  # Utils
  uuid        # UUID генерация
  mime_guess  # MIME type detection
  socket2     # Low-level socket options (multicast)

  ---
  Протокол DLNA — Краткий обзор

  ┌─────────────┐         SSDP M-SEARCH         ┌─────────────┐
  │   Control   │ ─────────────────────────────→│   Renderer  │
  │   Point     │                               │   (TV)      │
  │  (flingdlna)│ ←─────────────────────────────│             │
  └─────────────┘         SSDP Response         └─────────────┘
         │                                             │
         │  HTTP GET /description.xml                  │
         │ ───────────────────────────────────────────→│
         │                                             │
         │  SOAP SetAVTransportURI + Play              │
         │ ───────────────────────────────────────────→│
         │                                             │
         │                    HTTP GET /media/xxx      │
         │ ←───────────────────────────────────────────│
         │              (TV стримит файл)              │

  ---
  Файлы для изучения (в порядке важности)

  1. dlna-core/src/ssdp.rs — SSDP протокол, discovery, announcement
  2. dlna-server/src/http.rs — HTTP сервер, Range requests
  3. dlna-controller/src/transport.rs — AVTransport SOAP
  4. dlna-server/src/content_directory.rs — ContentDirectory Browse
  5. dlna-server/src/didl.rs — DIDL-Lite XML формат
