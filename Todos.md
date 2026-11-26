# Phonon Development Todos

## v0.2 — 核心短板补齐 ✅ 已完成

- [x] MP3 输出 (phonon-io: mp3lame-encoder, write_audio .mp3 路由, 128k-320k bitrate)
- [x] pydub 等效功能补齐 (phonon-core: overlay_at_position, reverse, repeat, loudness/dbfs/rms/adjust_volume_db)
- [x] VoiceConfig 情感扩展 (phonon-ai: emotion, style, language 字段)
- [x] Edge TTS Provider (phonon-ai: msedge-tts WebSocket, 免费无 API Key)
- [x] 并发 TTS 合成 (phonon-podcast: Semaphore + join_all, tts_concurrency 可配置, 默认 4)

## v0.3 — 播客 Agent 完整链路 ✅ 已完成

- [x] 动态范围压缩 (phonon-core: compressor — threshold/ratio/attack/release, 预设 voice/gentle/limiter)
- [x] 信号发生器 (phonon-core: Sine/Square/Sawtooth/WhiteNoise, generate_tone/generate_beep)
- [x] ElevenLabs TTS Provider (phonon-ai: voice_settings情感映射, 多模型支持)
- [x] LLM 脚本生成 (phonon-podcast: ScriptGenerator trait + OpenAiScriptGenerator, 话题→JSON脚本)
- [x] phonon-engine 播放控制 (pause/resume/stop/seek/volume, position/progress/state 查询)
- [x] phonon-studio 播客面板 (暂停/继续按钮, 依赖 phonon-podcast + phonon-ai)

## v0.4 — 进阶与生态 ✅ 已完成

- [x] STT 集成 (Whisper — phonon-ai: OpenAI Whisper with timed segments)
- [x] SRT 字幕自动生成 (phonon-podcast: subtitle module)
- [x] 多语言混合 TTS (phonon-podcast: multi-language segments)
- [x] phonon-video: 视频音轨提取与替换 (ffmpeg integration)
- [x] 播客自动章节标记 (phonon-podcast: chapters module)
- [x] Web API 服务 (phonon-server: axum REST API — /health, /api/tts, /api/podcast/*)
- [x] Workspace dependencies 统一管理 (workspace Cargo.toml)
- [x] criterion 性能基准测试 (phonon-core/benches/audio_algorithms.rs)

## v0.5 — 性能与质量 ✅ 已完成

- [x] Lanczos sinc 重采样 (phonon-core: resample_sinc windowed interpolation)
- [x] FLAC / OGG Vorbis / AAC 编码输出 (phonon-io: streaming encoders)
- [x] 实时音频效果链 (phonon-engine: AudioEffect trait + EffectChain)
- [x] phonon-cli 扩展, CI/CD, 集成测试, doctests

## v0.6 — Route B: 产品打磨 ✅ 已完成

- [x] 多频段参数化 EQ (phonon-core: eq.rs — EqBand, ParametricEq, presets)
- [x] rayon 并行处理 (phonon-core: 7 个算法文件，PAR_THRESHOLD 自动切换)
- [x] Studio UI 重构 (phonon-studio: 10 模块拆分, 频谱可视化, EQ 面板, 5 格式导出, 拖放)
- [x] Wasm 支持 (phonon-wasm: wasm-bindgen bindings + demo HTML, phonon-core parallel feature gate)
- [x] 插件架构 (phonon-core: DspPlugin trait, PluginParam, PluginRegistry, 5 内建插件)

## v0.7 — AI 播客生产就绪 + MCP 暴露 ✅ 已完成

> 详细规划见 [RoadMap.md](./RoadMap.md)

### Step 0 — WIP 清理基线 ✅ 已完成
- [x] 提交 phonon-ai 新功能 (SoundPreset / ReplicateSoundGen / ElevenLabs PCM 流式)
- [x] 提交 phonon-core SIMD ops 模块
- [x] 提交 phonon-studio plugin browser 面板
- [x] 提交 workspace rustfmt 2-space 整理
- [x] cargo fmt --check + cargo test --workspace 双绿

### Phase A — 音频质量补齐
- [x] **A1** LUFS 响度 (phonon-core/algorithms/lufs.rs) — ITU-R BS.1770-4, K-weighting, gating, LUFS-I/S/M, LRA, 真峰值 dBTP, normalize_lufs ✅
- [x] **A2** MP3 ID3v2 标签 + 章节 (phonon-io/mp3_tags.rs) — id3 crate, PodcastTags, CHAP/CTOC frames, write_mp3_tagged ✅
- [x] **A3** FLAC / OGG Vorbis Comment (phonon-io/tags.rs) — 通用元数据写入 ✅
- [x] **A4** 立体声 Pan (phonon-core/algorithms/pan.rs) — 等功率法则,pan_mono_to_stereo + pan_in_place ✅

### Phase B — 管线深度集成
- [x] **B2** TTS 结果缓存 (phonon-ai/cache.rs) — CachingTts 装饰器,blake3 hash 键,文件缓存 ✅
- [x] **B1** AI 音效生成接入管线 (phonon-podcast/pipeline.rs) — AssetSource::{File, Generated},自动 loop/trim ✅
- [x] **B3** Episode 出口 (phonon-podcast/export.rs) — Episode { audio, chapters, srt, tags },export_mp3/export_wav ✅

### Phase C — MCP 暴露层
- [x] **C1** phonon-mcp crate ✅
  - [x] 基础工具 (audio_load/save/info/list/drop)
  - [x] DSP 工具 (mix/trim/fade/normalize/normalize_lufs/volume_db/pan/resample)
  - [x] 测量工具 (audio_loudness)
  - [ ] AI 工具 (tts_synthesize/tts_list_voices/sound_generate) — 留作下游项目接入(provider 配置在调用方)
  - [ ] 管线工具 (podcast_generate/podcast_export) — 留作下游项目接入

### 收尾 ✅ 已完成
- [x] 集成测试 (phonon-tests: 端到端"脚本→tagged-mp3"流程 + MCP load/normalize_lufs/save 流程) ✅
- [x] README / AGENTS.md 更新 ✅
- [x] Todos.md 全部 DONE 标记 ✅

> v0.7 总计:`cargo test --workspace` 377 个单元/集成测试全部通过,`cargo fmt --check` 干净。新增 9 个独立功能 commit + 4 个 WIP 清理 commit + 2 个文档 commit。

## 远期(v0.8+)

- [x] 流式 TTS (边合成边播放) — phonon-engine `stream::play_stream` + `convert_for_device`,串接 `TtsProvider::synthesize_streaming` ✅
- [x] AudioEffect / DspPlugin 架构统一 (phonon-engine 适配 phonon-core plugin) — 内置 effects 改为 DspPlugin 薄封装并 `#[deprecated]`,新增 `EffectAsPlugin` 反向适配器,README 改为推荐 `add_plugin` 路径 ✅
- [x] wasm-pack 构建验证 + 在线 demo 部署 — `phonon-wasm/build.sh` + README,CI `build-wasm` job 自动跑 wasm-pack,`.github/workflows/wasm-pages.yml` 手动触发部署到 GitHub Pages ✅
- [x] SIMD 优化扩展 (其他热点 DSP 路径) — 新增 `simd_sum_sq` (loudness::rms / LUFS K-weighted) 与 `simd_mul_interleaved_pair` (pan_mono_to_stereo / pan_stereo_in_place),NEON + SSE2 + 标量 fallback,9 个新 SIMD vs 标量等价性测试 ✅
- [x] 发布到 crates.io — workspace version 0.1.0 → 0.7.0,补全各 crate description/keywords/categories/authors,新增 `scripts/publish-crates.sh` (按依赖顺序 dry-run/--execute) 与 `docs/publishing.md`,`cargo publish -p phonon-core --dry-run` 通过 ✅
- [x] VST3 / CLAP 插件兼容层 — 新增 `phonon-plugin` crate (`publish=false`),`PluginManifest` / `ParamInfo` / `PluginExport` / `ExportedPlugin` 把任何 `DspPlugin` 转成 host-friendly 描述符,`clap`/`vst3` 模块按 feature gate 提供工厂入口骨架,README 记录 SDK 选型与后续接入步骤。10 个测试全过 ✅

## v0.7.1 — 评估修复 ✅ 已完成

> 来源:[docs/evaluation-v0.7.md](./docs/evaluation-v0.7.md)。每项一个 commit,先写能红的测试再修。

### P0 — 安全与 CI
- [x] **F1** CI 转绿:修 clippy `-D warnings` 错误;`.cargo/config.toml` 移出版本库 ✅ (另加 CI doctest 步骤)
- [x] **F2** Server 加固:默认绑定 127.0.0.1、CORS 收紧、拒绝请求体内的文件路径资源、参数上界 + `validate()`、显式 body limit、去掉 `expect` ✅ (CORS 直接移除;并发/限流未做,留给反向代理)
- [x] **F3** MCP 路径沙箱:`PHONON_MCP_ROOT` 限定 load/save 根目录;registry 去掉 `expect` ✅ (另加 audio_save overwrite 保护)

### P1 — 结果错误
- [x] **F4** 管线:BGM 为空死循环;章节/SRT 计入 intro 与 crossfade ✅
- [x] **F5** Whisper `TranscribeOptions::default()` 默认开启 timestamps ✅
- [x] **F6** 重采样降采样时核支撑按 ratio 扩展 ✅ (10 kHz@48→16k 泄漏 -26 → -64 dB)
- [x] **F7** SIMD 在非 x86_64/aarch64 架构的标量 fallback ✅ (wasm32-wasip1 + node 实跑验证;CI 加 wasm32 lint)
- [x] **F8** 压缩器(compressor + CompressorPlugin)按帧计算时间常数 ✅ (改为声道联动,同帧同增益)
- [x] **F9** `channels == 0` 防护:相关算法返回错误;`read_audio` 无解码数据时报错 ✅ (AudioBuffer 字段私有化未做,需跨全 workspace 重构)
- [x] **F10** 引擎:回调不阻塞(try_lock)、控制线程不在锁内分配/释放、`seek` 帧对齐 ✅ (遗留:append_samples 锁内扩容、ParametricEqPlugin 回调内重建)

### P2 — 质量与架构
- [x] **F11** `HttpClient` 等 Debug 输出隐藏 API key ✅
- [x] **F12** 删除未使用依赖(mcp → ai/podcast/schemars,studio → ai/podcast) ✅
- [x] **F13** 效果抽象收敛:删 phonon-plugin、deprecated 包装、`EffectAsPlugin` ✅ (同时删 AudioEffect/add_effect,EffectChain 直接持有 DspPlugin)
- [x] **F14** phonon-io 编码器 feature 开关(aac 默认关)+ docs.rs 元数据 ✅ (只给 aac 加 feature;mp3/flac/ogg 暂无关闭需求,README 注明 LAME LGPL)
- [x] **F15** 文档同步(AGENTS / README / RoadMap / CHANGELOG 0.7) ✅ (CHANGELOG 加手写 v0.7.1 段;v0.6/v0.7 tag 未打,需维护者决定发布点)

## v0.7.2 — 评估遗留 ✅

> 来源同 [docs/evaluation-v0.7.md](./docs/evaluation-v0.7.md),v0.7.1 未覆盖的部分。

- [x] **G1** async 中的阻塞工作改 `spawn_blocking`(server 编码/解码/LUFS、pipeline 重采样与合并、各 provider 的 symphonia 解码、MCP DSP 工具)
  - DONE: pipeline 的 TTS 后处理整体走 `render()` + `spawn_blocking`;server 的 WAV 编码、LUFS、MP3 导出走阻塞池。
    未改:provider 的单段解码(每段几秒音频,毫秒级);MCP 请求串行处理,阻塞不影响其他请求——**重估条件**:MCP 改为并发处理请求
- [x] **G2** Edge TTS 连接/合成超时;MiniMax 限流与 Replicate 轮询的重试 + 退避;`HttpClient` 加 `connect_timeout`
  - DONE: `retry_transient`(3 次,500ms 起指数退避)用于 OpenAI/ElevenLabs/MiniMax/Edge 合成与 Replicate 轮询;MiniMax 1002/1039 → 429;Edge 改 async client + 60s 超时;所有 reqwest client 加 10s connect timeout
- [x] **G3** 测试中 `unsafe { env::remove_var }` 改为不依赖进程环境的构造方式(并行测试竞争)
  - DONE: 4 处构造函数统一走 `env_key()`;删除 3 个 `remove_var` 测试,改为对一个从不设置的变量名测试 `env_key`
- [x] **G4** 引擎:`append_samples` 不在锁内扩容;`ParametricEqPlugin` 不在 `process` 内重建 / 分配
  - DONE: samples 改 `RwLock`,扩容在读锁下拷贝、写锁只做交换;`ParametricEq::set_band` 原地换系数并保留滤波器状态(格式变化时仍重建)
- [x] **G5** Studio:EQ / 效果 / 播放前重采样 / 整轨 FFT 移出 UI 线程
  - DONE: `transform_audio` / `load_audio` 把处理与 waveform/FFT 放到 `spawn_blocking`;播放前的音量/重采样/声道转换同样移出。仅编译 + clippy 验证,GUI 未手动点测
- [x] **G6** 管线:`assemble_segments` 的 O(n²) 合并;`generate_episode` 改用并发 TTS(时间轴已按 span 计算,不再需要顺序合成)
  - DONE: 单个预分配 Vec 内追加 + 原地交叉淡化(与旧实现逐样本对拍);`synthesize_segments()` 供两个入口共用并发合成
- [x] **G7** LUFS 真峰值相位偏移(0.375/0.625/0.875 → 0.25/0.5/0.75)
  - DONE: sinc/窗中心改为 24;fs/4 中点 ISP 0.99 由 0.9766 (−0.12 dB 低读) → 0.9959
- [x] **G8** AAC 结束时未 flush 编码器尾部;`read_audio` 遇单个坏包即整体失败(应跳过 DecodeError)
  - DONE: AAC 按 `nDelay` 补静音帧冲出尾部(解码/输入能量比 0.872 → 0.997);解码错误跳过该包。顺带修复:FLAC STREAMINFO 最小块长导致 phonon 写出的 FLAC 无法被 `read_audio` 读回(d69a41d)。
    未覆盖:OGG 单页 CRC 损坏会在 symphonia 打开阶段失败——**重估条件**:symphonia 的 OggReader 支持容错打开
- [x] **G9** MCP:`AssetRegistry` 无容量上限;每个工具调用整块 clone PCM 只为构造元数据
  - DONE: 默认 1 GiB PCM 上限 + `RegistryFull`;9 处 `insert(buf.clone())` 改为 `store()` 先取元数据再 move
- [x] **G10** Replicate 轮询 / 下载未检查 HTTP 状态码;ElevenLabs `voice_id` 未转义直接拼进 URL
  - DONE: `ensure_success` 用于轮询与下载(下载也走重试);`tts_url` 以单个编码路径段放入 voice_id

## v0.7.3 — 架构项复评 ✅

> 对 [docs/evaluation-v0.7.md](./docs/evaluation-v0.7.md) 架构 3–6 逐条重新核实(2026-10-20)。
> 只做有当前证据的项;不做的写明重估条件。

- [x] **H1** 压缩器插件的 `ratio` 在线性幅度域计算,不是压缩比(阈值 0.5、ratio 20、输入 1.0 → 输出高出阈值 0.42 dB,实际约 14:1;ratio 4 实际约 3:1),与 `compress()` 的 dB 域结果不一致
  - 做法:`compress()` 与 `CompressorPlugin` 共用 compressor.rs 中同一个逐帧增益计算;插件参数不变(threshold 仍为 0–1 线性,内部换算 dB)
  - DONE: 共用 `compress_frames`;0.25/4:1 稳态输出 −7.18 dB → −9.03 dB,插件与 `compress()` 逐样本一致(e43457d)。同样参数下插件压得比以前重
- [x] **H2** `LowPassPlugin` / `HighPassPlugin` 状态固定 2 声道,第 3 声道起原样通过
  - 做法:状态按声道数分配,格式变化时重置
  - DONE: 状态改为按声道数的 Vec(声道数变化时重建);3 声道测试中第 3 声道的 Nyquist 残留 0.5 → <0.1
- [x] **H3** 字节解码五份复制(4 个 TTS provider + Replicate + server),每份都先写临时文件再读
  - 做法:phonon-io 增加 `decode_audio(bytes, ext_hint)`,内存解码;`read_audio` 共用同一解码循环
  - DONE: `decode_audio` 与 `read_audio` 共用 `decode()`;wav/mp3/flac/ogg 四种格式字节解码与文件读取逐样本一致;6 处副本删除,Edge 联网测试通过
- [x] **H4** `filters::filter()` 自带一份与 `ParametricEq` 相同的 DF-I biquad 循环
  - 做法:`filter()` 改为单频段 `ParametricEq`
  - DONE: `filter()` 构造单频段 `ParametricEq`,删 40 行;3 种滤波器、3 声道输入下与旧循环逐位一致
- [x] **H5** `OpenAiScriptGenerator` 不走 G2 的重试(429 / 5xx 直接失败),且 LLM 错误被归为 `PodcastError::Tts`
  - 做法:公开 `phonon_ai::retry_transient` 并在脚本生成中使用;错误归为 `Script`
  - DONE: `retry_transient` 公开(基础延迟仍为内部常量);本地服务器先 429 后 200 的测试中请求数 1 → 2;新增 `HttpClient::with_client` / `OpenAiScriptGenerator::with_client`
- [x] **H6** phonon-cli 声明了未使用的 phonon-wav 依赖
  - DONE: 已删除

不做(当前版本):
- **lufs.rs 的 biquad**:系数按 BS.1770 规范直接给出(DF-II 转置),与 RBJ cookbook 不同;可共享的只有 3 行差分方程。
  重估条件:出现第三个 DF-II 用户
- **phonon-wav 并入 phonon-io**:`write_wav` 本身就是 phonon-io 的 WAV 编码器,不是重复;`read_wav` 唯一调用方是 TTS 缓存,读取的是自己写出的 16-bit 文件,能力缺口不会触发。
  重估条件:`grep -rn "phonon_wav::read_wav" --include='*.rs' . | grep -v cache.rs` 有输出
- **AI trait RPITIT → dyn**:server 仍只用 Edge TTS(无需 API key)。重估条件:server 需要按请求或配置切换 provider
- **server 限流**:默认只监听 127.0.0.1。重估条件:默认监听地址改为非回环,或文档推荐公网部署

## v0.7.4 — 依赖收敛 ✅

- [x] **I1** reqwest 0.12 与 0.13 并存:phonon-ai / phonon-podcast 用 0.12,msedge-tts 0.4 依赖 0.13,两套 HTTP/TLS 栈都要编译
  - 做法:workspace 的 reqwest 升到 0.13,补齐 0.13 中改为可选的 feature;0.13 默认 TLS 为 rustls(0.12 为 native-tls),用联网 Edge TTS 测试与本地服务器测试验证
  - 验收:`cargo tree -d -e normal | grep -c '^reqwest'` 为 0
  - DONE: reqwest 0.13 + `query` feature;重复 reqwest 0 份,同时移除 native-tls / hyper-tls(512 → 508 个依赖);MiniMax 联网请求通过,workspace 测试 479 通过(a908888)

## v0.8 — 可配置模型 provider

> 设计与厂商实测见 [docs/providers.md](./docs/providers.md)。优先 MiniMax / Qwen / GLM / StepFun,
> 兼容 OpenAI 协议的自部署服务。每项都用真实 key 联网验证。

- [x] **J1** OpenAI 协议的 TTS / STT 适配厂商差异:GLM TTS 不支持 mp3;StepFun STT 拒绝 `verbose_json` 且无时间戳;GLM STT 默认即返回 segments
  - 做法:`OpenAiTts` 输出格式可配;`WhisperStt` 是否请求 `verbose_json` 可配，响应统一宽松解析;请求时间戳而 provider 不支持时报错
  - 验收:StepFun / GLM TTS,MiniMax / GLM / StepFun STT 联网通过
  - DONE: OpenAiTts::with_format;WhisperStt 增加 SegmentMode 与 with_path(MiniMax 路径为 speech_to_text,实测发现);联网:StepFun / GLM TTS,MiniMax / GLM / StepFun STT 通过。GLM 只有 `glm-asr` 返回时间戳,`glm-asr-2512` 只返回文本(9f874f3)
- [x] **J2** LLM 脚本生成适配：厂商关闭思考的参数不同;MiniMax 把 `<think>` 放进 content
  - 做法:`OpenAiScriptGenerator` 支持合并额外请求字段;解析前剥掉 `<think>…</think>` 与代码围栏
  - 验收：五家 LLM 各生成一份脚本并解析成功
  - DONE: `with_extra_body` + `json_payload`(剥 `<think>` 与代码围栏);五家各生成 4 段脚本并解析成功(DeepSeek 2.2 s 最快,MiniMax 20.9 s 最慢)(280f25a)
- [x] **J3** Qwen TTS(DashScope 原生：返回音频 URL,需二次下载)
  - DONE: `QwenTts`(生成 + 下载两次请求，均重试);`ensure_success` 移入 client.rs 共用;联网 Cherry / Ethan 通过，错误音色返回 400(4da733e)
- [x] **J4** Qwen STT(DashScope 原生，词级时间戳 → 按句组装 segments)
  - DONE: `QwenStt`(`parameters.format` 必填，实测缺失返回 400);词级时间按句末标点组装 segments;联网两句话片段得到 2 段(0.16–5.2 s / 6.76–8.39 s)(4ff50bb)
- [x] **J5** 配置模块：预置 + TOML + 按 key 自动选择 + `AnyTts` / `AnyStt` / `ChatEndpoint`
  - 验收：单元测试覆盖查找顺序、自动选择顺序、自定义 provider、缺字段报错
  - DONE: `Config` + 8 个预置 + 自定义 provider;`AnyTts` / `AnyStt` 用 enum 分发(v0.7.3「RPITIT → dyn」不做项的重估条件已触发，但 enum 足够，trait 不改);变异验证：调换优先级、去掉预置遮蔽均使测试变红;联网四家 TTS / STT 经配置通过(2ae1091)
- [x] **J6** TTS 超长文本按句切分(Qwen 600 / GLM 1024 / StepFun 1000 字上限)
  - 验收:Qwen 合成 1500 字文本成功
  - DONE: 按句切分;Qwen 预置 200 字(600 附近时长漂移 1.2–5 倍,实测)
- [x] **J7** STT 超长音频按时长切分(GLM 单次 ≤30 s),时间戳按偏移拼接
  - 验收:GLM 识别 36 s 音频成功且时间戳单调
  - DONE: GLM 36 s / Qwen 433 s 实测分段识别，时间戳单调
- [x] **J8** CLI 接入配置:tts / stt / subtitle / podcast / episode-export;`--config`;`providers` 子命令打印解析结果
  - DONE: --config/--provider/--model + providers 子命令;实测 stepfun/qwen tts、glm 字幕、qwen episode-export
- [x] **J9** server 的 TTS 按配置选择 provider(无 key 时仍为 Edge)
  - DONE: 启动时解析一次，经 router state 共享;实测 qwen 配置 200,坏配置拒绝启动
- [x] **J10** 播客管线每段按峰值归一化，不同音色响度相差 6 dB(Edge 4 个中文音色实测 -15.5 ~ -21.5 LUFS)
  - 做法：每段按 LUFS 归一到同一目标，过短片段退回峰值;整期 LUFS 归一
  - 验收：同一实验各音色差 ≤1 dB
  - DONE: 统一到共同 LUFS(受最尖峰段 -1 dBTP 约束);Edge 4 音色实测差 0.00 dB

## v0.8.1 — 真实出一期播客暴露的问题

> 2026-10-22 用 v0.8.0 出一期 18 段、979 字的中文双人播客:MiniMax LLM 写脚本(35 s),MiniMax TTS 合成并导出
> (36 s,211 s 音频)。内容完整：用 Qwen STT 回听，与脚本只差 3 处数字或同音字写法。以下问题按发现顺序排列。

- [x] **K1** `stt --json` 把 "Transcribing…" 打到 stdout,输出无法直接当 JSON 用
  - 验收:`phonon stt --json … | python3 -m json.tool` 成功
  - DONE: 进度提示改走 stderr;GLM 识别 two.wav 的 `--json` 输出经 `json.tool` 解析通过(8fd4bef)
- [x] **K2** 整期响度没达标：目标 -16 LUFS,实际 -18.5 LUFS,CLI 没有提示。原因是 `normalize_lufs` 用真峰值上限压低增益，没有限幅器;语音峰值比响度高约 17 dB,而 -16 LUFS / -1 dBTP 只留 15 dB
  - 做法:phonon-core 增加真峰值前瞻限幅，响度归一先加增益再限幅;CLI 打印实际达到的 LUFS / 真峰值
  - 验收：同一期重导出后积分响度在 -16 ±0.5 LUFS 内，真峰值 ≤ -1 dBTP;旧实现跑同一测试为红
  - DONE: 新增 `limit_true_peak`(4× 过采样峰值，前瞻 5 ms,释放 80 ms),`normalize_lufs` 先加足增益再限幅、最多 4 轮补偿;同一期重导出 PCM 为 -16.00 LUFS / -1.00 dBTP,编码成 MP3 后为 -16.27 / -1.24;旧实现跑新测试为红，把起点处理改回按 1.0 计算会让 3 个测试变红(559687b)
- [x] **K3** 字幕一段一条：平均 54 字、11.3 s,最长 79 字、15 s,无法当字幕显示(常规为每条 ≤2 行、几秒)
  - 做法：按句读标点把段切成短条，按字数比例分配该段时长
  - 验收：每条 ≤24 字;拿本期 Qwen STT 句级时间戳对照，切分点误差中位数 ≤0.3 s
  - DONE: 按 `split_text` 切成 ≤24 字(非 CJK 84 字)的条，按字数比例分配时长;段的起止去掉 TTS 首尾静音(MiniMax 约 0.3 s,原先每条都提前)。同期重导出 57 条，平均 17 字、3.4 s;对照 Qwen STT,切分点误差中位数 0.137 s,p90 0.93 s;关掉去静音时测试变红(3a72603)
- [x] **K4** 章节一段一个，标题是 "host" / "guest"(对话里每段都换人)
  - 做法:`Segment` 加可选 `chapter` 标题，脚本生成提示词让 LLM 标 3–6 个话题章节;有标注时按标注出章节，没有时保持原行为
  - 验收：重新生成本期，章节 3–6 个，标题是话题
  - DONE: `Segment.chapter` 可选;有标题时按标题分章节，否则仍按换人;提示词要求 3–6 章。重新生成同题脚本，MiniMax 标出 5 章(如「内容比蓝光更上头」),导出后 5 个章节，每章 33–63 s(a38ba00)
- [x] **K5** 换 provider 必须手改脚本 JSON 里的音色，而且没地方查某家有哪些音色(Qwen / GLM 报 "音色不存在")
  - 做法：预置带一组联网验证过的音色(第一个即默认),`phonon providers` 列出;`episode-export` / `podcast generate` 支持 `--voice 说话人=音色` 覆盖
  - 验收：同一份脚本用 `--voice` 在 Qwen、GLM 上导出成功
  - DONE: 预置改为音色表(首个为默认),MiniMax 6 / Qwen 7 / GLM 7 / StepFun 6 / Edge 4 个音色逐个联网通过，虚构音色五家都返回错误;`--voice 说话人=音色` 可重复;同一份脚本经 Qwen(Ethan/Cherry)、GLM(xiaochen/tongtong)导出成功(c5ccb26)
- [x] **K6** CLI 没有"选题 → 脚本"入口(只有库 API),`ScriptRequest` 默认音色是 OpenAI 的 alloy / echo
  - 做法:`phonon podcast script --topic … -o script.json`,用配置的 LLM;主持人和嘉宾音色取 TTS provider 音色表的前两个
  - 验收：用这条命令加 `episode-export` 从选题出一期，字幕、章节、响度都符合 K2–K4
  - DONE: `podcast script` + `--llm`;实跑暴露并修复两点:LLM 返回格式改为章节包段(原格式 7 次中 4 次 JSON 无效),max_tokens 4096→16384 + StepFun `reasoning_effort: low` + 截断时明确报错;五家共 12 次全部解析成功，各 3–5 章。端到端(MiniMax 写脚本 + Qwen TTS):195 s,4 章,-16.27 LUFS / -1.23 dBTP(MP3),字幕 57 条 ≤24 字、切分误差中位数 0.177 s;与 STT 的差异全是数字写法(五公里 vs 5)(41e8018)

## v0.8.1 — K6 那期暴露的后续问题

- [x] **L3** GLM 默认音色组合(tongtong / xiaochen)的性别没核实，可能是同性别的两个音色;官方文档不标性别
  - 做法：每个音色合成同一句，测中位基频(YIN);默认组合应一高一低
  - 验收：脚本对五家默认组合都通过;对一组已知同性别组合(GLM tongtong / chuichui)报错
  - DONE: 五家默认组合实测都是一高一低(差 52–189 Hz),预置不用改;GLM 除 xiaochen 外 6 个音色都在 276 Hz 左右。新增 scripts/voice_pitch.py(YIN 中位基频,差 <40 Hz 退出 1),对 tongtong/chuichui 为红;音高表写进 docs/providers.md(99805a7)
- [x] **L1** Qwen TTS 导出慢:195 s 的一期要 180 s(MiniMax 36 s)
  - 做法：先测时间花在哪(逐段串行？单段延迟？),再决定并发
  - DONE: 不是合成慢:release 构建整期 15 s(合成 11 s)。180 s = debug 构建后期处理约 45 s + 一个请求挂起等满 120 s 超时。真正的问题是 429 突发限流(4 次中 2 次同一请求连续两次 429,离失败只差一次)和挂起。重试 3→5 次(新测试旧值为红);Qwen TTS 单次超时 120→30 s,实跑一次挂起 30 s 后重试 2 s 成功，整期 45 s(f5c9e30)
- [x] **L2** 英文字幕不按句号切:`split_text` 对英文 "." 不断句，只能按 84 字符硬切
  - DONE: 句号后跟空白或文末才算句末(2.5 不切);超长子句在最后一个能放下的空格后切，不再切进单词;中文无空格，行为不变。新测试对旧代码为红，把按词切换回硬切也为红;实跑验证见 L5(15339da)
- [x] **L5** 英文节目没跑过端到端(`--language en-US`),同时验证 L2
  - DONE: MiniMax 写英文脚本(5 章 30 段)+ MiniMax TTS 导出 254 s,-16.00 LUFS。暴露 MiniMax RPM 限流导致 30 段导出失败(约 27 个请求后每 2–5 s 放行一个):429 重试改为最多 8 次(共 63.5 s),重导出 47 s 成功、最差一个请求第 6 次成功(b0bf309)。字幕 68 条 ≤84 字符 ≤5 s,42 条句末断开、0 条切进单词(旧算法同一脚本 31 条句末、7 条切进单词);STT 回听 688/688 词，差异只有数字写法;切分误差中位数 0.126 s,p90 0.72 s
- [x] **L4** BGM 路径没实跑(无音乐文件、无音效 provider):用合成或 CC0 音乐跑 `episode-export --bgm`,看闪避和响度
  - DONE: 用合成的 -12 / -24 LUFS 音乐床实跑：路径可用(MP3 解码、循环、混音、响度达标),但音乐比人声低 15.6 / 25.8 LU,随文件响度漂移。改为先把音乐调到人声积分响度再乘 bgm_volume,重导出两者都低 21.2 LU(默认 0.08 = 22 LU);旧实现跑新测试为红(c7e8a88)。未做：循环接缝交叉淡化(合成音乐接缝跳变 0.016,在正常步长内;重估条件：首次用真实音乐导出时，接缝前后各 1 s 的响度差 >6 LU 或能听出断句)
- [ ] **L6** BGM 循环接缝是硬拼接，没有交叉淡化。合成音乐测不出问题(接缝跳变 0.016,在正常采样步长内);真实音乐首尾常有淡入淡出或断在乐句中间
  - 重估条件：用第一首真实音乐导出时，接缝前后各 1 s 的响度差 >6 LU,或能听出断句

## v0.8.2 — 系统评估 P0 修复 ✅ 已完成

- [x] **M1** CI 回归止血：修 `filters.rs:164` 在 Rust 1.100 下的 `clippy::needless_range_loop`，清理 13 条 Cargo manifest 新告警
- [x] **M2** `simd_ops.rs` Soundness 修复：移除无保护 SSE3 `_mm_movehdup_ps` 改为基线 SSE `_mm_shuffle_ps`，补齐 `target_feature` 门控，`simd_add_clamp` 升级为 `assert_eq!`
- [x] **M3** `phonon-core` DSP 稳定性与边界防护：biquad 超 Nyquist / 零采样率 / 非正 Q 自动单位直通（修复 `ParametricEqPlugin` 在 16 kHz 语音下发散爆炸）；修 `resample` / `trim` / `overlay` / `silence` / `generators` 的 0 采样率、NaN/Inf、舍入越界与溢出 panic，补齐回归单测
- [x] **M4** `phonon-ai` SSRF 与下载上限：校验 `QwenTts` / `ReplicateSoundGen` 服务端返回 URL 为公网 HTTPS，Replicate 轮询锁定 `api.replicate.com` 同源，流式限制最大 100 MiB，日志隐藏预签名 query
- [x] **M5** `phonon-video` 加固：`ffmpeg`/`ffprobe` 统一 `-nostdin`、`Stdio::null()`、`kill_on_drop(true)`、超时、`-protocol_whitelist file,pipe`、`--` 与输出路径协议/同文件校验，直接传 `&Path`，临时文件改 `into_temp_path()`，读写移入 `spawn_blocking`
- [x] **M6** 解压炸弹与 OOM 防护：`phonon-io` 新增 `read_audio_limited`/`decode_audio_limited` 并复用 `SampleBuffer`；`phonon-mcp` 限制单行 4 MiB、限制 `audio_resample` 采样率范围、按注册表剩余配额解码并限 1024 句柄；`phonon-server` 提前拒绝 `AssetSource::Generated`、校验 `bgm_volume`/`normalize_target`、限制 `/api/lufs` 解码样本数并增加并发/超时中间件
- [x] **M7** `CachingTts` 原子写盘与指纹隔离：改同目录临时文件 + `rename` 并移入 `spawn_blocking`；`TtsProvider` 新增 `cache_fingerprint()` + 长度前缀/显式 Option tag 哈希；补齐 MiniMax `format` 配置传递与并发缓存单测

## v0.8.3 — 系统评估 P1 修复与性能提质 ✅ 已完成

- [x] **N1** `phonon-engine` 与 `DspPlugin` 实时安全：`DspPlugin::prepare()` 在 `add_plugin` 预分配滤波状态；`PluginParam`/`set_volume` 忽略非有限值；修 `play`/`clear`/`append_samples`/`write_data` 竞态与 `streaming` 泄漏，`effect_count` 改无锁原子；新增 `I16`/`U16` 设备支持
- [x] **N2** `phonon-core` LUFS 提速与声学边界：`measure_loudness` 合并为单遍 K-weighting、短时步长对齐 EBU 100ms、re-export `limit_true_peak`、消除冗余二次限幅；修 `compressor` Inf 污染与 `fade`/`crossfade`/`mix` 帧对齐及尾部一致性
- [x] **N3** `phonon-ai` 可靠性：`retry_transient` 加抖动、`env_key` 自动 trim、`WhisperStt` 接入重试；`Endpoint.api_key` 用 `ApiKey` 脱敏；修 `AnyTts::synthesize_streaming` 绕过 `max_chars`、STT `max_secs==0`、Qwen 10 MB 字节切分与非限流错误误重试
- [x] **N4** `phonon-podcast` 与 `phonon-cli`：`generate_to_file` 按扩展名编码、`preview_segment` 补齐 `segment.language`、`level_segments` 限最大 +24 dB、`validate()` 校验 `PipelineConfig`、修章节越界与孤立标点字幕；CLI tracing 固定写 stderr、`lufs --target` 缺 `--output` 报错
- [x] **N5** `phonon-io` / `phonon-wav` / `phonon-server` / `phonon-mcp`：`write_mp3` 改为 8192 帧流式分块编码、`attach_flac_tag` 替换旧封面、`phonon-wav` 校验 0 采样率/声道；`podcast_preview_handler` 强类型与正确状态码、同步 `phonon-server/README.md`；MCP 校验 JSON-RPC 2.0 请求对象、`audio_pan` 范围校验、`audio_info` 有限值安全及缺 `PHONON_MCP_ROOT` 告警

## v0.9.0 — Phonon Studio: AI Podcast & Voice Studio (Desktop Phase 1 MVP)

- [x] **S1** 架构设计文档（`docs/studio-architecture.md`）
- [x] **S2** 底层最小接口扩展：`phonon-engine` 插件参数实时更新与链同步、`phonon-ai` 预置供应商/音色查询、`phonon-podcast` LUFS 母带选项与离线/缓存段拼装
- [x] **S3** `phonon-studio` 纯 Rust 可测试内核：`StudioProject` IR（兼容 `PodcastScript`）、Undo/Redo 历史、分段缓存状态、母带响度与插件烘烤导出逻辑 + 单元测试
- [x] **S4** AI Podcast Studio 三栏桌面界面：供应商/角色/母带配置、AI 脚本起草、逐段 `CachingTts` 试听与局部重合成、整期拼装、BS.1770 LUFS/dBTP 仪表、可跳转章节/SRT 预览与标签化一键导出
- [x] **S5** Audio Lab 次级工作台修复与整合：实时插件参数同步、懒加载播放器链同步、修复暂停切轨误续播、共享实时 Seek/音量 Transport、导出支持烘烤插件链
- [x] **S6** Tailwind v4 构建、全工作区质量门禁（`fmt`/`clippy -D warnings`/`cargo test --workspace`）与文档更新

## v0.9.1 — Phonon Studio Phase 2（文本剪音频 + 本地语音增强 + MCP 工程联动）

- [x] **P2-1** `phonon-core` 纯 Rust 语音后期内核：`splice_ranges`（微交叉淡化非破坏性拼接）、`detect_speech_regions`（离线静音切分）与 `voice_enhance`（STFT 频谱降噪 + 广播人声链）
- [x] **P2-2** `phonon-mcp` 工程与语音工具扩展：新增 `audio_enhance_voice`、`audio_splice` 及受 `PHONON_MCP_ROOT` 约束的 5 个 `studio_*` 原子协同编辑工具
- [x] **P2-3** `phonon-studio` Phase 2 内核：段落录音挂载与 STT 词/句对齐（无 Key 自动退化为离线静音切分）、口癖自动识别、非破坏性裁剪 + 语音增强接入拼装管线、MCP 文件热同步逻辑 + 单元测试
- [x] **P2-4** `phonon-studio` Phase 2 界面：段内「改文本即剪音频」可点击词条/一键删口癖/录音增强开关、Audio Lab 一键播音腔与自动去静音、顶栏 MCP Live Sync 与配置面板
- [x] **P2-5** Tailwind v4 构建、全工作区质量门禁与文档更新（`studio-architecture.md`、`mcp-tools.md`、`Todos.md`）

## v0.9.0 收官 — 应用内麦克风直录、生态补齐与 CI/发布准备

- [x] **R1** `phonon-engine` 实时麦克风采集：`AudioRecorder`（支持 F32/I16/U16、无锁原子 VU 峰值与时长、30 分钟上限、帧对齐与无硬件单测）
- [x] **R2** `phonon-studio` 应用内直录集成：播客分段直录（`● Record Mic` / `■ Stop & Attach` 实时红灯 VU 电平条、原子落盘至 `recordings/` 并自动对齐增强）+ Audio Lab 顶栏直录
- [x] **R3** `phonon-mcp` 补齐 `audio_eq` / `audio_compress`（工具总数达 23 个）与 `phonon-io` 的 `mp3` / `ogg` 可选 C 编码器 feature 隔离（支持 `--no-default-features` 纯 Rust MIT/Apache-2.0 构建）
- [x] **R4** `phonon-tests` 清单清理与跨 crate 集成测试扩展（FLAC/OGG 标签回读、Phase 2 端到端语音管线、MCP 异步双工 stdio 23 工具测试）、CI 新增 `build-studio` 与纯 Rust `phonon-io` 检查，以及 workspace `0.9.0` 版本号与发布文档同步

## v0.10.0 — Phonon Studio UI 迁移至 `dioxus-shadcn`（双仓双向验证）

> 规划见 `studio_dioxus_shadcn_migration_plan.md`。坚持直接消费 crates.io 已发布的 `dioxus-shadcn` (`0.7.0+`) 与 `dxui` CLI；若在真实 Desktop WebView 迁移与验收中发现上游缺陷，先在 `/Users/hal/arch/dioxus-ui` 撰写 RFC 与 `TODOs.md`（`M226+`），按其发版流程推送到 crates.io 后再回到 `phonon-studio` 升级验收。

- [x] **U1** 基建接入、语义主题与 CLI 验证：`phonon-studio` 设为 `rust-version = "1.88"`，引入 crates.io `dioxus-shadcn 0.7.0`（按需 features），实跑已发布 `dxui init` 生成样式与 `@source`，将 Broadcast Mastering Console 暗色配色映射到 RFC 0051 语义 Token，跑通 Tailwind v4 构建、`clippy`、测试与桌面冒烟
  - DONE (`6dd5000`): crates.io `dioxus-shadcn 0.7.0` + `dxui 0.7.0` 实跑接入完成；Tailwind v4 59ms 构建、clippy 0 告警、16 单测全绿。记录首条真实 CLI 观察：`dxui init` 在 Crate 模式下仍会创建空白 `src/components/ui/mod.rs`（`FB-studio-01` 候选）
- [x] **U2** 迁移 `Header` 与 `Transport`：`Button` / `Tabs` / `Badge` / `Separator` / `Tooltip` / `Kbd` / `Slider`，并实测 macOS Desktop WebView 下 8Hz 播放头刷新与拖拽 Seek/Volume 流畅度
  - DONE (`5a66f48`): `Header` 与 `Transport` 全面迁至 `dioxus-shadcn` (`Tabs` / `Button` / `Badge` / `Separator` / `Tooltip` / `Slider`) 与语义颜色 Token；Tailwind v4 构建 59ms、clippy 0 告警、16 单测全绿（桌面交互流畅度随 U5 整窗验收实测）
- [x] **U3** 迁移 `AudioLab` 及其 4 个功能面板（`EffectsPanel` / `EqPanel` / `PluginBrowser` / `ExportPanel`），实测 10+ 并发 `Slider`、`NativeSelect`/`Select`、`Checkbox`/`Switch`、`Card`、`Empty`
  - DONE (`b5c8d48`): 完成 `AudioLabView`（`ResizablePanelGroup` + `Empty`）、`WaveformView`、`InfoPanel` 及 4 个 DSP/导出面板迁移，全部通过 Tailwind v4（58ms）、fmt、clippy 与 16 单测
- [ ] **U4** 拆分并迁移 `PodcastStudioView`（将 1917 行拆为 `components/studio/` 子模块），接入 `ResizablePanelGroup` 三栏、`Dialog`/`Sheet`（AI Draft & MCP Sync）、`Tabs`（Underline Inspector）、`Card` / `Input` / `Textarea` / `Button` (`loading`) / `Badge` / `Sonner`
- [ ] **U5** Desktop WebView 全链路交互验收与 `FB-studio-*` 上游实测缺陷清单沉淀（区分 `dioxus-ui` 真实缺陷与业务适配）
- [ ] **U6** 消费上游 `dioxus-shadcn` 修复版本（升级 crates.io 版本、重跑 `dxui init` 验证 `@source` 自动更新、清理临时绕过、更新文档与全工作区门禁）
