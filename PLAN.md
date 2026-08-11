# План полностью настраиваемого JS/TS-форматера на Rust 😎🌬

  ## 1. Что именно строим

  Делаем самостоятельный formatter, который ставится из npm и сразу работает как обычная проектная утилита:

  pnpm add -D <tool>
  pnpm exec <tool> --init
  pnpm exec <tool> --check .
  pnpm exec <tool> --write .

  Это уже не узкая утилитка, братец, а полноценный formatter engine 🍎🍏. До npm v0.1 он обязан:

  - полностью принимать JS, TS, JSX и TSX;
  - не иметь fallback, который молча оставляет неизвестные AST-узлы нетронутыми;
  - сохранять смысл программы, комментарии и suppression directives;
  - быть идемпотентным;
  - поддерживать обязательный типизированный JSONC-конфиг;
  - давать собственный канонический стиль, без обещаний совместимости с Prettier или Oxfmt;
  - предоставлять npm CLI и асинхронный Node.js API;
  - держать весь горячий флоу внутри Rust без JS-callback’ов на AST-узлах 💨.

  ### Зависимости и жёсткие границы

  Используем только опубликованные registry-пакеты, без Git-зависимостей и копирования исходников:

  oxc_allocator = "=0.144.0"
  oxc_ast = "=0.144.0"
  oxc_ast_visit = "=0.144.0"
  oxc_diagnostics = "=0.144.0"
  oxc_parser = { version = "=0.144.0", default-features = false }
  oxc_span = "=0.144.0"
  oxc_syntax = "=0.144.0"

  dprint-core = {
    version = "=0.68.3",
    default-features = false,
    features = ["formatting"]
  }

  Опубликованные Oxc crates сейчас доступны как 0.144.0 (https://crates.io/crates/oxc_parser/0.144.0). Живые oxc_formatter и oxc_formatter_core не используем: в
  текущем Oxc они имеют publish = false — crates/oxc_formatter/Cargo.toml:11 и crates/oxc_formatter_core/Cargo.toml:11.

  dprint-core 0.68.3 (https://crates.io/crates/dprint-core/0.68.3) отвечает только за IR, условные переносы, indentation и окончательную печать. Все JS/TS-решения,
  комментарии и пользовательские policies принадлежат нашему проекту 😎👍.

  Зафиксировать:

  - Rust edition 2024;
  - MSRV 1.95, потому что этого требует выбранная версия Oxc;
  - MIT license;
  - точные версии Oxc и dprint-core обновлять отдельными grouped PR;
  - каждый апдейт зависимостей прогоняет snapshots, semantic verification и benchmarks.

  ## 2. Архитектура и публичные интерфейсы

  ### Workspace

  Собрать ровную трёхслойную забивочку:

  crates/
    formatter/   Pure Rust formatter engine
    cli/         Config discovery, file walking, check/write/stdin
    napi/        Node-API boundary

  packages/
    npm/         Public JS API, CLI launcher, schema and generated types

  npm/
    <platform>/  Generated platform-specific native packages

  - formatter ничего не знает о npm, файловом обходе или NAPI.
  - cli занимается диском, .gitignore, ближайшими конфигами, параллельной обработкой и exit codes.
  - napi только переводит Rust-результаты и ошибки через Node-API.
  - JS launcher лениво загружает platform addon и вызывает Rust CLI.
  - Имя репозитория становится basename для npm package, bin-команды и <tool>.jsonc; scoped npm package сохраняет тот же короткий bin.

  ### Основной formatter-флоу

  Для каждого файла:

  1. Определить SourceType через SourceType::from_path.
  2. Поддержать .js, .mjs, .cjs, .jsx, .ts, .mts, .cts, .tsx и declaration-файлы.
  3. Отделить UTF-8 BOM и вернуть его после печати.
  4. Создать отдельный oxc_allocator::Allocator.
  5. Запустить oxc_parser с TokensParserConfig и такими настройками:
      - preserve_parens: false;
      - enable_ident_hashes: false;
      - allow_return_outside_function: true;
      - allow_v8_intrinsics: true;
      - regex AST parsing выключен;
      - JSX включён для JavaScript-файлов так же, как делает formatter Oxc.

  6. При любой parser diagnostic остановиться и оставить исходник неизменным.
  7. Отклонить Flow как неподдерживаемый язык v0.1.
  8. Через oxc_ast_visit::Visit::enter_node/leave_node построить индекс узлов, родителей, spans, token boundaries и комментариев.
  9. Рекурсивно превратить AST в собственный тонкий Doc-adapter поверх dprint_core::formatting::PrintItems.
  10. Напечатать результат с resolved config.
  11. Применить выбранный line ending, final newline и вернуть BOM.
  12. При verifyAst: true заново распарсить результат с теми же options и сравнить Program через ContentEq.
  13. При output diagnostic или content_ne вернуть внутреннюю ошибку и ничего не записывать.
  14. Вернуть None, если байты не изменились, иначе новый String.

  ### Doc-adapter

  Типы dprint-core не должны расползаться по formatter-модулям, пацанчик 🌬. Внутренний adapter предоставляет:

  text
  token
  concat
  space
  hard_line
  soft_line
  line_or_space
  indent
  group
  force_flat
  conditional
  line_suffix
  measured

  Adapter переводит их в PrintItems, Signal, Condition, LineNumber и ConditionReevaluation.

  Это позволяет:

  - менять printer dependency без переписывания всех AST-formatters;
  - единообразно строить группы;
  - измерять, стал ли конкретный узел многострочным;
  - вставлять spacing на основании окончательного результата, а не исходного текста.

  PrintItems создаются и потребляются только внутри closure dprint_core::formatting::format: наружу или между thread’ами они не передаются.

  ### Контекст и policies

  Перед горячим обходом raw config превращается в полностью заполненный ResolvedConfig. В formatter-флоу не остаётся Option, строкового парсинга или JSON.

  FormatContext
    source
    source_type
    tokens
    comments
    node_index
    emitted_comments
    parent_stack
    resolved_config

  Policies разделить по реальному поведению:

  CorePolicy
  ObjectPolicy
  ArrayPolicy
  ImportPolicy
  StatementSpacingPolicy

  Никакого публичного plugin ABI, JS callback API или произвольного selector DSL в v0.1 не добавлять 💨.

  ### Rust API

  pub struct ResolvedConfig {
      // Publicly readable only through typed getters.
  }

  pub enum FormatError {
      Parse,
      UnsupportedSource,
      InvalidConfig,
      Verification,
      Internal,
  }

  pub fn resolve_config(config: FormatConfig) -> Result<ResolvedConfig, FormatError>;

  pub fn format_text(
      file_name: &Path,
      source_text: &str,
      config: &ResolvedConfig,
  ) -> Result<Option<String>, FormatError>;

  Option<String> означает:

  - None — файл уже отформатирован;
  - Some(code) — получен изменённый результат.

  ### npm API

  Публичный ESM API:

  export interface FormatConfig {
    // Generated from the Rust JSON Schema.
  }

  export async function format(
    fileName: string,
    sourceText: string,
    config: FormatConfig,
  ): Promise<string>

  - config обязателен в программном API.
  - API не ищет конфиги по файловой системе.
  - NAPI использует worker task, чтобы CPU-работа не блокировала Node event loop.
  - Ошибка содержит стабильный code: PARSE_ERROR, UNSUPPORTED_SOURCE, CONFIG_ERROR, VERIFICATION_ERROR или INTERNAL_ERROR.
  - TypeScript-типы генерируются из той же JSON Schema, что проверяет Rust.

  ## 3. Formatting contract и конфиг

  ### Обязательный JSONC

  CLI ищет только <tool>.jsonc:

  - поиск начинается от форматируемого файла;
  - поднимается до ближайшего VCS root;
  - ближайший config побеждает;
  - результаты кэшируются по директориям;
  - --config <path> отключает discovery для всего запуска;
  - отсутствие config — ошибка с подсказкой npx <tool> --init;
  - --init создаёт полный шаблон и не перезаписывает существующий файл.

  Поля optional, но сам файл обязателен. --init записывает все публичные настройки явно. Неизвестные ключи считаются ошибкой, чтобы опечатанная забивочка не
  курилась молча 🤣.

  Минимальная схема:

  {
    "$schema": "./node_modules/<package>/configuration_schema.json",

    "lineWidth": 100,
    "indentStyle": "space",
    "indentWidth": 2,
    "lineEnding": "preserve",

    "quoteStyle": "single",
    "semicolons": "always",
    "trailingCommas": "multiline",
    "bracketSpacing": true,
    "arrowParentheses": "always",
    "finalNewline": true,
    "verifyAst": true,

    "objects": {
      "layout": "auto",
      "propertyLayout": "auto",
      "whenArrayElement": "inherit"
    },

    "arrays": {
      "layout": "auto",
      "elementLayout": "auto",
      "objectElements": "inherit"
    },

    "imports": {
      "specifierLayout": "auto"
    },

    "statementSpacing": [
      {
        "previous": {
          "kind": "import",
          "lineShape": "singleLine"
        },
        "next": {
          "kind": "const",
          "lineShape": "multiLine"
        },
        "scope": "topLevel",
        "blankLines": 1
      }
    ],

    "ignorePatterns": []
  }

  ### Точные значения

  Базовые настройки:

  - lineWidth: 1..=320, default 100;
  - indentStyle: space | tab, default space;
  - indentWidth: 0..=24, default 2;
  - lineEnding: preserve | lf | crlf, default preserve;
  - preserve использует первый найденный line ending, а при его отсутствии — LF;
  - quoteStyle: single | double, default single;
  - semicolons: always | asNeeded, default always;
  - trailingCommas: none | multiline | all, default multiline;
  - arrowParentheses: always | asNeeded, default always;
  - verifyAst: default true на всей ветке v0.x.

  Collection policies:

  - layout: auto | preserve | singleLine | multiLine;
  - propertyLayout и elementLayout: auto | preserve | onePerLine;
  - objects.whenArrayElement: inherit | multiLine;
  - arrays.objectElements: inherit | onePerLine;
  - imports.specifierLayout: auto | preserve | onePerLine.

  singleLine и lineWidth считаются мягкими пожеланиями: line comment, multiline literal или синтаксическая безопасность могут принудительно разломать строку.

  Statement selector:

  kind:
    any | import | export | const | let | var |
    function | class | type | interface | enum |
    namespace | other

  lineShape:
    any | singleLine | multiLine

  scope:
    any | topLevel | block

  Spacing rules:

  - проверяются сверху вниз;
  - первое совпадение побеждает;
  - blankLines: 0 означает обычный один line break без пустой строки;
  - shape определяется по окончательно напечатанному коду узла;
  - leading/trailing comments в shape не входят;
  - comments остаются прикреплены к своему statement bundle;
  - правило никогда не двигает комментарий через пользовательский код.

  Для final shape formatter оборачивает code body statement’а двумя LineNumber markers. Spacing condition читает resolved start/end lines обоих соседей и
  использует reevaluation, если данные следующего узла ещё не напечатаны.

  ### Собственный стиль

  Общие правила, которые обязаны лечь в docs/formatting-contract.md и snapshots:

  - Пустые delimited-конструкции печатаются компактно.
  - Непустая sequence остаётся в строке, пока помещается.
  - При разломе opening delimiter заканчивает строку, элементы получают один indent, closing delimiter выравнивается с началом конструкции.
  - Blocks всегда сохраняют исходную AST-структуру: formatter не добавляет и не удаляет {}.
  - Binary и logical chains при разломе переносятся перед оператором.
  - Ternary при разломе печатает ? и : на отдельных indented-строках.
  - Call/new arguments и function parameters при разломе идут по одному на строку.
  - Member/call chains ломаются перед ., ?. или computed segment.
  - Named import/export specifiers при разломе идут по одному на строку.
  - Variable declaration при разломе размещает declarators по одному на строку.
  - Non-block control-flow body печатается на следующей indented-строке; formatter не меняет AST добавлением braces.
  - else, catch и finally остаются рядом с закрывающей brace предыдущего block.
  - TS union/intersection при разломе использует leading |/&.
  - JSX attributes при разломе идут по одному на строку; meaningful JSX whitespace не нормализуется вслепую.
  - Numeric, regexp и template raw text сохраняются; строковые literals подчиняются quoteStyle.
  - Parentheses вычисляются собственной precedence/associativity таблицей; исходные лишние parentheses не сохраняются.
  - При semicolons: asNeeded отдельный ASI guard добавляет leading semicolon перед опасными statement starts.
  - Formatter никогда не сортирует declarations, imports, object properties или union members.

  ### Комментарии и suppression

  Отдельный comment engine строит leading/trailing/dangling attachment из Oxc comments, tokens, spans и parent index.

  Инварианты:

  - исходный comment text печатается байт-в-байт;
  - каждый comment ID должен быть отмечен ровно один раз;
  - после root formatting оставшийся или повторно напечатанный comment превращается в INTERNAL_ERROR;
  - line comment использует line-suffix semantics и не пересекает line boundary;
  - dangling comments внутри пустых delimiters принадлежат контейнеру;
  - comments не пересекают пользовательские tokens;
  - // <tool>-ignore и /* <tool>-ignore */ сохраняют следующий AST-узел как raw source slice;
  - ignored node всё равно участвует в общем AST verification.

  ## 4. Порядок реализации

  ### Срез 1 — рабочий вертикальный кальджубасик 🌬

  - Создать workspace, exact dependencies, formatting crate, NAPI crate и npm package.
  - Реализовать config types, schema generation и полный --init template.
  - Сделать минимальный Doc-adapter над dprint-core.
  - Провести один файл через parser → AST → IR → printer → NAPI → JS API.
  - Поддержать identifiers, literals, variable declarations, простые objects/arrays/imports.
  - Добавить один реальный object policy и statement-spacing rule.
  - Для неподдержанного AST возвращать явную development error.
  - Локально проверить pnpm exec <tool> и импорт format().

  Срез считается готовым, когда локальный npm package форматирует пример, повторный запуск не меняет результат, а verifyAst проходит.

  ### Срез 2 — correctness spine

  - Построить token/node/comment index.
  - Реализовать comment tracker и suppression.
  - Ввести expression precedence, associativity и parent-position model.
  - Реализовать required-parentheses engine.
  - Добавить semicolon/ASI engine.
  - Включить output reparse + ContentEq.
  - Закрыть BOM, line endings, hashbang, directives и final newline.
  - Создать единый fixture harness: expected output, idempotency, ContentEq, comments emitted once.

  После этого дальнейшие AST-formatters используют только готовые общие механизмы, а не свои локальные костыли 🍎🍏.

  ### Срез 3 — полный JavaScript

  Закрывать exhaustive dispatch семействами:

  1. literals, identifiers, templates и regexp;
  2. unary/update/binary/logical/assignment/conditional expressions;
  3. member, call, new, optional chaining и tagged templates;
  4. arrays, objects, spreads и patterns;
  5. functions, arrows, parameters и generators;
  6. statements, loops, switch, try/catch, labels и control statements;
  7. classes, fields, methods, accessors, private names и decorators;
  8. imports, exports и module declarations.

  Все enum matches должны быть exhaustive, без _ => raw_source. Обновление Oxc с новым variant обязано ломать компиляцию, пока formatter для него не добавлен.

  ### Срез 4 — TypeScript и JSX

  - TypeScript types, type parameters/arguments, declarations, signatures и modifiers.
  - Interfaces, type aliases, enums, namespaces, declare, import-equals и export-assignment.
  - TS assertions, as, satisfies, non-null и instantiation expressions.
  - JSX elements, fragments, attributes, spreads, text и expression containers.
  - TSX ambiguity и generic-arrow cases.
  - Declaration files .d.ts, .d.mts, .d.cts.
  - Отдельная fixture-категория для comments и parentheses на каждом TS/JSX семействе.

  Срез закрывается только тогда, когда release build не имеет unsupported AST branches.

  ### Срез 5 — CLI и проектный config

  - Реализовать --init, --config, --check, --write, --stdin-filepath, --threads, --no-verify, --help, --version.
  - Без mode-флага разрешать stdout только для stdin или одного файла.
  - Для директории или нескольких файлов требовать --check либо --write.
  - --check возвращает:
      - 0, если всё чисто;
      - 1, если нужны изменения;
      - 2, если есть parse/config/internal errors.

  - Directory walking уважает .gitignore, ignorePatterns, .git и node_modules.
  - Явно переданный unsupported file возвращает ошибку; найденный во время directory walk пропускается.
  - Multi-file formatting выполняется через Rayon с immutable Arc<ResolvedConfig>.
  - Диагностики сортируются по path перед выводом, чтобы результат не прыгал между запусками.
  - --write пишет через temporary sibling file и atomic rename, сохраняя permissions.

  ### Срез 6 — npm-доставка

  Сделать Oxfmt-подобную упаковку:

  - root npm package содержит ESM API, bin/<tool>, schema и .d.ts;
  - platform packages подключаются как optionalDependencies;
  - bin является тонким #!/usr/bin/env node launcher;
  - binding загружается лениво;
  - engines.node фиксируется как ^20.19.0 || >=22.12.0;
  - Node-API обеспечивает ABI-stable addon boundary — официальная документация (https://nodejs.org/api/n-api.html).

  Матрица v0.1:

  x86_64-apple-darwin
  aarch64-apple-darwin
  x86_64-pc-windows-msvc
  aarch64-pc-windows-msvc
  x86_64-unknown-linux-gnu
  aarch64-unknown-linux-gnu
  x86_64-unknown-linux-musl
  aarch64-unknown-linux-musl

  Каждый target проходит runtime smoke test на соответствующем окружении. Release job собирает артефакты, раскладывает их через napi artifacts, выполняет
  собственный dry run napi pre-publish --dry-run, затем публикует platform packages и root package. Эти стадии и их побочные эффекты зафиксированы в NAPI-RS
  artifacts (https://napi.rs/docs/cli/artifacts) и pre-publish (https://napi.rs/docs/cli/pre-publish).

  ## 5. Проверки, release gates и границы

  ### Автоматические проверки

  Unit tests:

  - Doc primitives и conditional layouts;
  - config defaults, ranges, unknown fields и JSON paths в diagnostics;
  - source type и BOM/newline handling;
  - statement classification, first-match precedence и final-shape conditions;
  - expression precedence, associativity и ASI guards.

  Fixture/snapshot tests:

  - минимум один fixture на каждый Oxc AST variant;
  - flat и broken layout для каждой sequence;
  - comments: leading, trailing, dangling, line, block, multiline и suppression;
  - strings, templates, regexps и directives;
  - object/array/import policy matrix;
  - все сочетания statement kind, scope и line shape;
  - JS, TS, JSX, TSX и declaration files.

  Каждый валидный fixture автоматически проверяет:

  output parses without diagnostics
  input Program content_eq output Program
  every source comment emitted exactly once
  format(output) == output

  CLI E2E:

  - missing config;
  - --init и отказ от overwrite;
  - nearest config и --config;
  - stdout, stdin, check и write;
  - .gitignore и ignorePatterns;
  - parse error не меняет файл;
  - exit codes;
  - deterministic diagnostics;
  - paths с Unicode и пробелами.

  NAPI/npm:

  - TypeScript API types;
  - lazy binding load;
  - missing platform package diagnostic;
  - установка из packed tarball;
  - node_modules/.bin/<tool>;
  - runtime smoke test каждого из восьми артефактов.

  Fuzzing:

  - seed corpus из fixtures;
  - мутации валидного JS-family source;
  - ни одного panic;
  - для успешно распарсенного input обязательны parseable output, ContentEq и idempotency.

  Performance:

  - Criterion benchmarks отдельно для parse, node/comment indexing, IR generation, print и verify;
  - размеры: маленький файл, 50 KB, 1 MB и смешанный проект;
  - сравнить verifyAst: true/false;
  - измерять npm cold start отдельно от Rust throughput;
  - до стабильного baseline benchmarks отчётные, затем regression gate фиксируется относительно committed baseline;
  - никакой JS↔Rust коммуникации внутри обработки AST.

  ### Условия npm v0.1

  - все JS/TS/JSX/TSX AST variants поддержаны exhaustively;
  - нет raw fallback, кроме явного <tool>-ignore;
  - весь fixture corpus проходит semantic verification и idempotency;
  - CLI/API packages устанавливаются и запускаются на восьми targets;
  - benchmarks опубликованы в release notes;
  - verifyAst включён по умолчанию.

  ### Явно вне v0.1

  Чтобы кальджубасик не расползся по всей кальянной 🤣🌬:

  - JSON/JSONC formatting;
  - JS plugins и callbacks;
  - arbitrary Rule DSL;
  /tmp/<tool>-formatter-implementation-plan.md
