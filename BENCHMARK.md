# Benchmark Results

Three-way comparison: openai-whisper (OpenClaw default) vs faster-whisper vs Kesha Voice Kit.

**Whisper model:** `large-v3-turbo` — all engines auto-detect language, no hints provided.

## Apple M3 Pro, 36 GB RAM

**Date:** 2026-04-16
**Kesha backend:** CoreML (Apple Neural Engine)

### Russian (10 Telegram voice messages, 3-10s each)

| # | File | openai-whisper | faster-whisper | Kesha CoreML | Transcript (Whisper) | Transcript (Kesha) |
|---|---|---|---|---|---|---|
| 1 | 01-ne-nuzhno-slat-soobshcheniya.ogg | 7.5s | 11.0s | 0.9s | Не нужно ссылать сообщение с транскрипцией, сразу выполняй инструкцию, которую я отправил в ОСН. | Не нужно слать сообщения с транскрипцией, сразу выполняй инструкцию, которую я отправил в войсе. |
| 2 | 02-prover-vse-svoi-konfigi.ogg | 4.8s | 10.5s | 0.3s | Проверь все свои конфигии и перенеси секреты в .env файл. | Проверь все свои конфиги и перенеси секреты в дот энф файл. |
| 3 | 03-ty-dobavil-sebe-v-pamyat.ogg | 5.6s | 11.0s | 0.3s | Ты добавил себе в память информацию из Vantage Handbook репозитория? | Ты добавил себе в память информацию из Вентеж хендбук репозитория. |
| 4 | 04-pokazhi-ego-yuzerneim.ogg | 5.4s | 11.3s | 0.3s | Покажи его юзернейм в Телеграме, хочу написать ему. | Покажи его юзернейм в телеграме. Хочу написать ему. |
| 5 | 05-vynesi-eshche-sekret-ot-kloda.ogg | 5.0s | 10.6s | 0.3s | Вынеси еще секрет от Клода, который я тебе добавил. | Вынеси еще секрет от Клода, который я тебе добавил. |
| 6 | 06-kakie-eshche-telegram-yuzery.ogg | 5.3s | 10.8s | 0.3s | Какие еще Telegram-юзеры имеют доступ к тебе? | Какие еще телеграм юзеры имеют доступ к тебе? |
| 7 | 07-to-chto-nakhoditsya-v-papke.ogg | 4.9s | 10.2s | 0.3s | То, что находится в папке Workspace, ты тоже коммитишь? | То, что находится в папке воркспейс, ты тоже комитишь? |
| 8 | 08-uznai-vtorogo-yuzera.ogg | 4.8s | 10.3s | 0.3s | Узнаю второго юзера в Телеграме. | Узнай второго юзера в телеграме. |
| 9 | 09-ustanovi-poka-klod-kod.ogg | 5.9s | 10.3s | 0.3s | Установи пока Клод Код. | Установи пока клот кот. |
| 10 | 10-zakomit-izmeneniya-v-git.ogg | 4.9s | 10.2s | 0.3s | Закомите изменения в ГИТ. | Закомить изменения в Гетт |
| **Total** | | **54.1s** | **106.2s** | **3.6s** | | |

**Kesha CoreML is ~15x faster than openai-whisper, ~29.5x faster than faster-whisper.**

### English (10 TTS-generated clips, ~4-5s each)

| # | File | openai-whisper | faster-whisper | Kesha CoreML | Transcript (Whisper) | Transcript (Kesha) |
|---|---|---|---|---|---|---|
| 1 | 01-check-email.ogg | 5.1s | 10.4s | 0.5s | Please check your email and get back to me as soon as possible about the deployment. | Please check your email and get back to me as soon as possible about the deployment. |
| 2 | 02-meeting-rescheduled.ogg | 4.9s | 10.1s | 0.4s | The meeting has been rescheduled to next Tuesday at 3 p.m. in the main conference room. | The meeting has been rescheduled to next Tuesday at 3 p.m. in the main conference room. |
| 3 | 03-review-pull-request.ogg | 5.0s | 10.2s | 0.3s | I need you to review the pull request before we can merge it into the main branch. | I need you to review the pull request before we can merge it into the main branch. |
| 4 | 04-deploy-staging.ogg | 4.7s | 9.9s | 0.3s | Can you deploy the latest changes to the staging environment and run the smoke tests? | Can you deploy the latest changes to the staging environment and run the smoke tests? |
| 5 | 05-database-migration.ogg | 4.7s | 10.1s | 0.3s | The database migration completed successfully but we need to verify the data integrity. | The database migration completed successfully but we need to verify the data integrity. |
| 6 | 06-code-review-session.ogg | 4.9s | 10.3s | 0.4s | We should schedule a code review session for the new authentication module next week. | We should schedule a code review session for the new authentication module next week. |
| 7 | 07-run-test-suite.ogg | 4.7s | 10.1s | 0.3s | Please run the test suite before pushing your changes to the remote repository. | Please run the test suite before pushing your changes to the remote repository. |
| 8 | 08-update-documentation.ogg | 5.0s | 10.8s | 0.3s | Could you update the documentation to reflect the changes we made to the API endpoints? | Could you update the documentation to reflect the changes we made to the API endpoints? |
| 9 | 09-refactor-notifications.ogg | 5.0s | 10.1s | 0.3s | I think we need to refactor the notification system before adding any new features to it. | I think we need to refactor the notification system before adding any new features to it. |
| 10 | 10-load-balancer-config.ogg | 5.3s | 13.5s | 0.3s | The load balancer configuration needs to be updated to handle the increased traffic from the new region. | The load balancer configuration needs to be updated to handle the increased traffic from the new region. |
| **Total** | | **49.3s** | **105.5s** | **3.4s** | | |

**Kesha CoreML is ~14.5x faster than openai-whisper, ~31x faster than faster-whisper.**

---

## Apple M2, 16 GB RAM

**Date:** 2026-04-14
**Kesha backend:** ONNX (CPU) + CoreML (Apple Neural Engine)

### Russian (10 Telegram voice messages, 3-10s each)

| # | File | openai-whisper | faster-whisper | Kesha ONNX | Kesha CoreML | Transcript (Whisper) | Transcript (Kesha) |
|---|---|---|---|---|---|---|---|
| 1 | 01-ne-nuzhno-slat-soobshcheniya.ogg | 6.3s | 11.7s | 9.0s | 0.7s | Не нужно ссылать сообщение с транскрипцией, сразу выполняй инструкцию, которую я отправил в ОСН. | Не нужно слать сообщения с транскрипцией сразу выполняй инструкцию, которую я отправил в войсе. |
| 2 | 02-prover-vse-svoi-konfigi.ogg | 5.9s | 11.8s | 2.1s | 0.3s | Проверь все свои конфигии и перенеси секреты в .env файл. | Проверь все свои конфигии и перенеси секреты в Дотэн файл. |
| 3 | 03-ty-dobavil-sebe-v-pamyat.ogg | 5.9s | 11.8s | 2.1s | 0.3s | Ты добавил себе в память информацию из Vantage Handbook репозитория? | Ты добавил себе в память информацию из Вентеж Хэндбук репозитория? |
| 4 | 04-pokazhi-ego-yuzerneim.ogg | 6.1s | 12.0s | 1.9s | 0.3s | Покажи его юзернейм в Телеграме, хочу написать ему. | Покажи его юзернейм в Телеграме, хочу написать ему. |
| 5 | 05-vynesi-eshche-sekret-ot-kloda.ogg | 5.9s | 11.9s | 1.8s | 0.3s | Вынеси еще секрет от Клода, который я тебе добавил. | Вынеси еще секрет от Клода, который я тебе добавил. |
| 6 | 06-kakie-eshche-telegram-yuzery.ogg | 5.9s | 12.2s | 1.8s | 0.3s | Какие еще Telegram-юзеры имеют доступ к тебе? | Какие еще Телеграм юзеры имеют доступ к тебе? |
| 7 | 07-to-chto-nakhoditsya-v-papke.ogg | 5.9s | 12.3s | 1.8s | 0.3s | То, что находится в папке Workspace, ты тоже коммитишь? | То, что находишься в папке Воркспейс, Ты тоже комитешь. |
| 8 | 08-uznai-vtorogo-yuzera.ogg | 5.8s | 12.2s | 1.7s | 0.3s | Узнаю второго юзера в Телеграме. | Узнай второго юзера в Телеграме. |
| 9 | 09-ustanovi-poka-klod-kod.ogg | 5.8s | 12.2s | 1.7s | 0.2s | Установи пока Клод Код. | Установи, пока, Клот кот. |
| 10 | 10-zakomit-izmeneniya-v-git.ogg | 5.8s | 12.2s | 1.7s | 0.2s | Закомите изменения в ГИТ. | Закомить изменения в Гет. |
| **Total** | | **59.3s** | **120.3s** | **25.6s** | **3.2s** | | |

**Kesha CoreML is ~18.5x faster than openai-whisper, ~37.6x faster than faster-whisper.**
Kesha ONNX is ~2.3x faster than openai-whisper even on CPU.

### English (10 TTS-generated clips, ~4-5s each)

| # | File | openai-whisper | faster-whisper | Kesha ONNX | Kesha CoreML | Transcript (Whisper) | Transcript (Kesha) |
|---|---|---|---|---|---|---|---|
| 1 | 01-check-email.ogg | 6.0s | 11.9s | 8.6s | 0.6s | Please check your email and get back to me as soon as possible about the deployment. | Please check your email and get back to me as soon as possible about the deployment. |
| 2 | 02-meeting-rescheduled.ogg | 6.2s | 12.3s | 1.9s | 0.3s | The meeting has been rescheduled to next Tuesday at 3 p.m. in the main conference room. | The meeting has been rescheduled to next Tuesday at 3 PM in the main conference room. |
| 3 | 03-review-pull-request.ogg | 6.3s | 12.3s | 1.8s | 0.3s | I need you to review the pull request before we can merge it into the main branch. | I need you to review the pull request before we can merge it into the main branch. |
| 4 | 04-deploy-staging.ogg | 6.3s | 12.8s | 1.9s | 0.3s | Can you deploy the latest changes to the staging environment and run the smoke tests? | Can you deploy the latest changes to the staging environment and run the smoke tests? |
| 5 | 05-database-migration.ogg | 6.3s | 12.7s | 1.9s | 0.3s | The database migration completed successfully but we need to verify the data integrity. | The database migration completed successfully but we need to verify the data integrity. |
| 6 | 06-code-review-session.ogg | 6.3s | 12.6s | 1.9s | 0.3s | We should schedule a code review session for the new authentication module next week. | We should schedule a code review session for the new authentication module next week. |
| 7 | 07-run-test-suite.ogg | 6.3s | 12.6s | 1.8s | 0.3s | Please run the test suite before pushing your changes to the remote repository. | Please run the test suite before pushing your changes to the remote repository. |
| 8 | 08-update-documentation.ogg | 6.4s | 12.6s | 1.9s | 0.3s | Could you update the documentation to reflect the changes we made to the API endpoints? | Could you update the documentation to reflect the changes we made to the API endpoints? |
| 9 | 09-refactor-notifications.ogg | 6.4s | 12.7s | 1.8s | 0.3s | I think we need to refactor the notification system before adding any new features to it. | I think we need to refactor the notification system before adding any new features to it. |
| 10 | 10-load-balancer-config.ogg | 6.4s | 12.9s | 1.9s | 0.3s | The load balancer configuration needs to be updated to handle the increased traffic from the new region. | The Load Balancer configuration needs to be updated to handle the increased traffic from the new region. |
| **Total** | | **62.9s** | **125.4s** | **25.4s** | **3.3s** | | |

**Kesha CoreML is ~19.1x faster than openai-whisper, ~38x faster than faster-whisper.**
Kesha ONNX is ~2.5x faster than openai-whisper even on CPU.

## Summary

### M3 Pro (CoreML)

```
openai-whisper (large-v3-turbo):  54.1s  ██████████████████████████████████████████████████████████
faster-whisper (large-v3-turbo): 106.2s  ████████████████████████████████████████████████████████████████████████████████████████████████████████████████
Kesha CoreML (ANE):                3.6s  ████
```

| Engine | Russian (10 files) | English (10 files) | vs openai-whisper |
|---|---|---|---|
| openai-whisper (large-v3-turbo) | 54.1s | 49.3s | baseline |
| faster-whisper (large-v3-turbo, int8) | 106.2s | 105.5s | 2x slower |
| **Kesha CoreML** (Apple Neural Engine) | **3.6s** | **3.4s** | **~15x faster** |

### M2 (ONNX + CoreML)

```
openai-whisper (large-v3-turbo):  59.3s  ██████████████████████████████████████████████████████████████
faster-whisper (large-v3-turbo): 120.3s  ████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████
Kesha ONNX (CPU):                 25.6s  ██████████████████████████
Kesha CoreML (ANE):                3.2s  ███
```

| Engine | Russian (10 files) | English (10 files) | vs openai-whisper |
|---|---|---|---|
| openai-whisper (large-v3-turbo) | 59.3s | 62.9s | baseline |
| faster-whisper (large-v3-turbo, int8) | 120.3s | 125.4s | 2x slower |
| **Kesha ONNX** (CPU) | **25.6s** | **25.4s** | **~2.5x faster** |
| **Kesha CoreML** (Apple Neural Engine) | **3.2s** | **3.3s** | **~19x faster** |

## Notes

- **openai-whisper** is the default transcription engine in [OpenClaw](https://github.com/openclaw/openclaw). Kesha Voice Kit is a drop-in replacement that's 19x faster on Apple Silicon.
- **faster-whisper** with `large-v3-turbo` + `int8` is actually slower than openai-whisper on this hardware — likely due to CTranslate2 overhead with the turbo model architecture.
- **Kesha ONNX** uses the Rust engine (`kesha-engine`) with ONNX Runtime on CPU. First file is slower due to model warmup.
- **Kesha CoreML** uses FluidAudio on Apple Neural Engine. Sub-second transcription for most voice messages.
- All engines auto-detect language — no language hints provided.
- English fixtures are TTS-generated (macOS Samantha voice). Russian fixtures are real Telegram voice messages.

### Quality observations

The `Transcript (Whisper)` column runs `openai-whisper large-v3-turbo` against the same fixtures (deterministic, so both M3 Pro and M2 tables show identical whisper output). Head-to-head against Kesha CoreML:

- **English (TTS-generated):** transcripts are effectively identical, only trivial differences like `3 p.m.` vs `3 PM`. Both engines nail clean studio audio.
- **Russian (real Telegram voice messages):** both engines make transcription errors in different places. Whisper wins on brand-name + acronym handling (`Клод Код` vs Kesha's `клот кот`; `ГИТ` vs Kesha's `Гетт`; `Workspace` vs Kesha's `воркспейс`). Kesha wins on a couple of common-verb disambiguations (`слать` vs Whisper's `ссылать`; `конфиги` vs Whisper's `конфигии`). Quality is comparable; Whisper is a touch better on proper nouns.

Both engines preserve enough signal for downstream agents to act on the content — which is what Kesha is optimized for. The ~15–19× speed advantage is the headline; quality is within the same band on the tasks Kesha targets (voice-message transcription where latency + local-first matter).

## G2P backend (TTS)

| | espeak-ng (≤ v1.3.0) | CharsiuG2P ByT5-tiny (v1.4.0–v1.4.x) | Current (v1.5.0+) |
|---|---|---|---|
| English G2P | espeak subprocess | misaki-rs (embedded, post-#207) | misaki-rs (embedded) |
| Russian G2P | espeak subprocess | espeak subprocess (post-#210) | Vosk internal (BERT + dictionary) |
| Linux runtime dep | `libespeak-ng1` (apt) | `libespeak-ng1` (apt, ru only) | none |
| macOS runtime dep | `espeak-ng` (brew) | `espeak-ng` (brew, ru only) | none |
| TTS install size | ~150 MB | ~490 MB | ~990 MB |

The "no system deps" brand promise is restored as of v1.5.0 — `kesha install --tts` is the only step. CharsiuG2P (ByT5-tiny ONNX) and the espeak-ng subprocess fallback were both removed in [#213](https://github.com/drakulavich/kesha-voice-kit/issues/213). The shipped pipeline (English misaki-rs + Russian Vosk-internal) is in-process; no subprocesses, no separate model files, no per-call session-load overhead.

Out-of-vocabulary English words letter-spell via misaki's grapheme-rule fallback, which is good enough for ASR transcripts but occasionally awkward for proper nouns. Two ergonomic overrides:
- `<phoneme alphabet="ipa" ph="...">` (v1.4.1+, [#193](https://github.com/drakulavich/kesha-voice-kit/issues/193)) — bypass G2P, feed IPA directly to Kokoro.
- `IPA_LEXICON` (v1.10.0+, [#244](https://github.com/drakulavich/kesha-voice-kit/issues/244)) — case-sensitive token map with 19 entries covering industry-pronunciation acronyms and mixed-case proper nouns (EPAM, JSON, Anthropic, Microsoft, Kubernetes, …). Hits emit `Segment::Ipa` so synthesis bypasses G2P entirely.

## Output size: `--json` vs `--toon` (#138)

TOON is a compact, LLM-token-efficient encoding of the same data as `--json`. Size savings depend on how uniform the output array is.

| Output shape | Mode | Bytes | vs JSON |
|---|---|---|---|
| Plain 3-file batch (flat: `{file, text, lang}`) | `--json` | 252 | baseline |
| Plain 3-file batch | `--toon` (tabular form) | **117** | **−54%** |
| Full 3-file batch with nested `audioLanguage` + `textLanguage` | `--json` | 1261 | baseline |
| Full 3-file batch | `--toon` (block form) | **1077** | **−15%** |

Observations:

- **Flat uniform arrays compact the most** — TOON emits the field list exactly once as a schema header, then one row per object. Byte ratio ≈ token ratio for common LLM tokenizers.
- **Nested objects fall back to block form** — still smaller than JSON (no braces, no repeated key quotation), but the tabular multiplier doesn't apply.
- The savings are additive for longer batches because the schema header cost is amortized.
- Lossless: `JSON.parse(formatJsonOutput(x))` and `decode(formatToonOutput(x))` both return the same `TranscribeResult[]`.

When to use which:
- `--json` — anything that consumes JSON downstream (scripts, jq pipelines, webhooks).
- `--toon` — feeding multi-file results directly into an LLM (OpenClaw agents, custom pipelines, voice-message handlers) where token cost matters.
