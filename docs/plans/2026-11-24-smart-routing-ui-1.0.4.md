# Locus 1.0.4 智能分流页 UI 重构任务书

- 编写：2026-11-24，`droid`（只写方案，未改代码）
- 执行者：由用户指定的 Agent
- 基线：`main` 分支，提交 `e1fc2fa`，标签 `v1.0.3`，`versionName=1.0.3` / `versionCode=4`
- 目标版本：`versionName=1.0.4` / `versionCode=5`（只顺位加 1）
- 用户授权（2026-11-24）：本任务书第 7 节的全部模拟器验收通过后，提交源码、推送到 GitHub，并在 GitHub Release 发布 APK。任何一项验收未通过，都不得改版本号、不得提交、不得推送、不得发布。

## 1. 目标

智能分流页的视觉和交互要与节点页一致。策略卡太大，与节点页不协调。策略详情的框也太大。本轮只改智能分流页及其子页，不改节点页和设置页的外观。

## 2. 用户已确认的决定

1. 保留「路由规则」卡片（`smart_rules`）。
2. 「立即更新规则」放到页头，做成 38 dp 按钮。
3. 「自动选择范围」「引用规则更新」「管理节点汇总组」三张卡片从主页移除。
4. 策略卡按节点行的结构重做，高度与节点行相近。
5. 策略卡的点击逻辑：
   - 点击延迟：测试延迟。目标是分组时，测试全组并切换到延迟最低的节点。现有 `vm.service.testTarget(value,key)` 已实现这个行为（`runtime/ServiceClient.kt:241`），不得改它的语义。
   - 点击节点或节点分组（第二行）：打开分流目标选择器 `TargetPicker`。
   - 点击卡片其他区域：打开策略详情子页。子页显示「分流规则」和「选择应用」。
6. 删除现有的行内展开。
7. 自定义规则全部放进同一张分组卡。
8. 对话框中点「确定」后立即保存。只有「批量编辑文本」保留显式「保存」。

## 3. 设计规范（照搬现有页面，不新造）

| 项 | 取值 | 来源 |
|---|---|---|
| 列表左右边距 | 16 dp | `ui/ZaneApp.kt:298` |
| 卡片间距 | 8 dp | `ui/ZaneApp.kt:298` |
| 策略卡最小高度 | 60 dp | 节点行 `ui/ZaneApp.kt:330` |
| 行标题 | 14 sp Medium | 节点行 `ui/ZaneApp.kt:334`；`DESIGN.md` 第 3 节 |
| 副标题 | 12 sp，`onSurfaceVariant` | 节点行 `ui/ZaneApp.kt:335` |
| 延迟文字 | 12 sp，等宽，三档颜色，普通 `Text` + `clickable`，不用 `TextButton` | 节点行 `ui/ZaneApp.kt:337-345` |
| 页头按钮 | 48 dp 触控区内的 38 dp、8 dp 圆角、`surfaceElevated` + 1 dp `divider` 描边 | `ui/ZaneApp.kt:242-253` |
| 子页分组卡 | 一张 `UiCard` 内多行；行高 56 dp；0.5 dp 分隔线；有图标时分隔线左缩进 50 dp | `ui/UiSettings.kt:44-48`、`ui/UiSettings.kt:222-225` |
| 动效 | 只用有限 `tween`；不得用无限动画 | `DESIGN.md` 第 5 节 |

策略卡过高的原因：延迟 `TextButton` 设了 48 dp 最小高（`ui/UiSmart.kt:130`），目标 `TextButton` 有 Material 3 默认 40 dp 最小高（`ui/UiSmart.kt:139`），再加行最小高 68 dp 和上下内边距，实测约 107 dp。节点行约 62 dp。证据截图：`reports/s25-emulator-20261124-074855.pKoQWj/smart-after-scroll.png`、`nodes-initial.png`。

## 4. 改动清单

### A. 智能分流主页（`ui/UiSmart.kt` 的 `SmartPanel`，`ui/ZaneApp.kt:208-210` 页头）

1. 页头 `actions` 在 `SmartMenu` 前加「立即更新规则」按钮：
   - 图标 `Icons.Outlined.Download`，调用 `vm.updateAllSmartRules()`。
   - testTag 沿用 `smart_update`。
   - 样式使用 C 节抽出的 38 dp 按钮。
2. `SmartMenu` 的 `UiMenu` 改为 `circle=true`，与节点页 `node_menu` 样式一致。tag 仍为 `smart_menu`。
3. `SmartMenu` 增加两项，放在「路由规则」之后：
   - 「规则自动更新间隔」：打开现有 `ChoiceDialog`，键 `rulesUpdateInterval`，选项与现有代码相同（`ui/UiSmart.kt:110-111`）。
   - 「连接后检查规则」：键 `rulesUpdateDelay`，选项同上。
   - 菜单已有「节点汇总组」，保留，作为原「管理节点汇总组」卡片的替代入口。
4. 删除以下列表项：
   - `smart-groups-section`（「分流节点组」小节标题）
   - `smart-source`（「自动选择范围」卡片）
   - `smart-update`（「引用规则更新」卡片）
   - `smart-merges`（「管理节点汇总组」卡片）
5. 「应用策略」小节标题右侧显示「自动选择 · %s 个节点」（数值来自 `summary.autoNodeCount`），12 sp，次要文字色。需要给 `UiSection` 增加可选的右侧文字参数，或在本页写一个带右侧文字的版本。新增文案必须加进 `ui/UiTranslations.kt` 的三种语言。
6. 不改：返回箭头 `main_back`、「添加自定义应用组」按钮、VPN 提示的位置和 tag。

### B. 策略卡（替换 `SmartPolicyRow`、`TitleWithLatency`、`NodeWithTarget`、`LatencyText`）

布局：

```
[图标]  YouTube                              37 ms
        香港专线订阅 → HK 香港 · IEPL 专线 56x
```

1. 外层仍是 `UiCard`，保留拖动排序（`drag.modifier`、`animateItem`）和 `smart_card_$key`。
2. 行最小高度 60 dp。左侧图标 22 dp，放在 48 dp 宽的列中居中，与节点行单选按钮同一竖线。
3. 第一行：标题 14 sp Medium，单行省略；右侧为延迟。
   - 延迟是普通 `Text`，tag `smart_latency_$key`，点击调用 `vm.service.testTarget(value,key)`。
   - 延迟永远完整显示，不得被截断。标题让位。
4. 第二行：「目标名 → 最低延迟节点名」，12 sp，次要文字色，单行省略。
   - 整行可点击，点击打开 `TargetPicker`，tag `smart_target_$key`。
   - 节点名部分保留 tag `smart_node_$key`。
   - 没有最低延迟节点时，只显示目标名。
5. 卡片其他区域可点击，打开策略详情子页，tag `smart_$key`。
6. 删除行内展开（`ui/UiSmart.kt:90-101`）和 `expanded` 状态。
7. 延迟三档颜色：
   - 在 Rust `smart_panel_summary`（`rust/locus_core/src/runtime_logic.rs`）的每条策略结果中增加数值字段 `lowestPing`（毫秒；无结果或失败时与节点 `ping` 字段同语义）。
   - JNI（`rust/locus_core/src/jni_bridge.rs`）、Kotlin `SmartPolicyRowSummary`（`ui/SmartPolicyLine.kt:8-12`）同步增加字段。
   - 颜色判断使用 C 节抽出的共享函数，测试中状态用 `skinPrimaryDark`。
   - Rust 单元测试和 `app/src/test/.../SmartPolicyLineTest.kt` 必须覆盖新字段。
8. 性能约束，不得破坏：
   - 保留 `remember(data,testing){smartPanelSummary(...)}` 缓存，滑动中不得新增 JNI 调用或 `AppData.toJson()`。
   - 所有 `item` 保留 `contentType`。
   - 不得使用 `BoxWithConstraints` 或其他子组合布局。

### C. 共享组件（`ui/UiComponents.kt`）

1. 抽出 38 dp 页头按钮（例如 `CompactActionButton(icon,description,tag,onClick)`）。替换 `ui/ZaneApp.kt:242-253` 两处和 `UiMenu` 的 `circle` 非 prism 分支。prism 皮肤保留原 `FilledIconButton` 分支。
2. 抽出延迟颜色函数，替换 `ui/ZaneApp.kt:337-344`。节点页外观不得变化。

### D. 策略详情子页（新，替代行内展开和现有 `RuleSourcePage`）

页面用 `UiPageList`，标题为策略名称。

```
←  YouTube                                    ⋮
┌────────────────────────────────────────────┐
│ 香港专线订阅 → HK 香港 · IEPL 专线 56x        │  只读，全文换行
└────────────────────────────────────────────┘
应用分流
┌────────────────────────────────────────────┐
│ ▦ 选择应用          12 个有效应用          › │
└────────────────────────────────────────────┘
分流规则
┌────────────────────────────────────────────┐
│ ☰ 规则来源  内置兼容规则组 / URL       ⤓   › │
│   ⚠ 兼容提醒（仅在有时显示）                  │
└────────────────────────────────────────────┘
自定义规则 · 8 条                      + 添加
┌────────────────────────────────────────────┐
│ googlevideo.com                    后缀  › │
│ ────────────────────────────────────────── │
│ youtube.com                        后缀  › │
│ ────────────────────────────────────────── │
│ 批量编辑文本                             › │
└────────────────────────────────────────────┘
```

1. 顶部只读信息：完整显示目标名和当前节点名，可换行，不可点击。
   - 目标名 tag `smart_goal_$key`，节点名 tag `smart_current_$key`。
   - 原因：`TargetLatencyUiTest`（第 203-205 行）验证长名称能完整显示。主页策略卡是单行省略，完整名称必须在这里显示。
2. 「⋮」菜单（tag `smart_detail_menu`）：
   - 「删除规则源」：沿用现有确认对话框，确认按钮 tag `rule_source_delete_confirm`。
   - 自定义应用组另有「重命名」和「删除应用组」，逻辑沿用 `ui/UiSmart.kt:98-99`、`ui/UiSmart.kt:118`。
3. 「应用分流」卡：一行「选择应用」，tag `smart_apps_$key`，打开现有 `AppsEditor`。
4. 「分流规则」卡：
   - 「规则来源」行（tag `rule_source_row`）：副标题显示 URL，或「内置兼容规则组」/「自定义域名规则」。点击打开 URL 对话框。
   - 行尾 ⤓ 图标按钮执行「获取并替换」，tag `rule_source_replace`。获取期间显示 22 dp 进度圈，替换图标。
   - 兼容提醒：只在 `ruleCompatibilityWarnings` 非空时显示，12 sp，`skinWarning` 色，tag `rule_source_compatibility`。
   - 错误：12 sp，错误色，tag `rule_source_error`。
   - 删除原说明段落（`ui/UiSmart.kt:326`）。
5. URL 对话框（`UiAlertDialog`）：
   - 输入框 tag `rule_source_url`；「在线目录 / Karing 搜索」按钮 tag `rule_catalog`，打开现有目录对话框。
   - 「确定」（tag `rule_source_url_confirm`）：校验 URL 后立即保存 `smartUrl.$key`。校验失败时，对话框不关闭，并显示错误。
6. 自定义规则：
   - 小节标题「自定义规则 · N 条」，右侧「添加」文字按钮，tag `rule_domain_add`。
   - 全部规则放进同一张 `UiCard`。每行 48 dp，0.5 dp 分隔线。
   - 每行左侧显示域名或原始规则，右侧短标签显示类型：「完整」「后缀」「关键词」「原始」。tag `rule_domain_$index`。
   - 点击域名规则，打开现有类型选择和域名对话框。点击原始规则，打开批量编辑。
   - 域名对话框「确定」（`rule_domain_confirm`）后立即保存 `smartRules.$key`。
   - 最后一行「批量编辑文本」，tag `rule_source_raw`。打开后显示文本框 `rule_source_rules` 和「保存」按钮 `rule_source_save`。批量编辑保留显式保存。
7. 「获取并替换」的并发防护保留：获取期间 URL 或规则被修改时，不覆盖修改（`ui/UiSmart.kt:268`），并继续使用 `applySmartRuleUpdate`。

## 5. testTag 契约

- 保留且位置不变：`smart_rules`、`smart_menu`、`smart_add_custom`、`smart_vpn_hint`、`smart_card_$key`、`main_back`、`page_list`。
- 保留但移动位置：`smart_update`（页头）、`smart_$key`（卡片点击区）、`smart_latency_$key`、`smart_target_$key`（第二行）、`smart_node_$key`、`smart_apps_$key`（详情页）、`smart_goal_$key`、`smart_current_$key`（详情页顶部）、全部 `rule_*` tag（详情页）。
- 删除：`smart_source`、`smart_merges`。
- 新增：`smart_detail_menu`、`rule_source_row`、`rule_source_url_confirm`。

## 6. 测试迁移

| 测试 | 必须改的内容 |
|---|---|
| `TargetLatencyUiTest` | 第 80 行 `smart_merges` 改为经 `smart_menu` →「节点汇总组」进入。第 171-174、203-205 行：点击 `smart_youtube` 后进入详情页，再断言 `smart_current_youtube` 和 `smart_goal_youtube`。 |
| `SmartDomainPickerTest` | 每次 `rule_domain_confirm` 后断言已保存到 store。删除对 `rule_source_save` 的点击。 |
| `RuleKeyboardLayoutTest` | 确认 `smart_speed` → `rule_source_raw` → `rule_source_rules` 路径可用。 |
| `FunctionFeedbackTest` | 确认页头 `smart_update` 可点击且有反馈。 |
| `UiLanguageRepairInstrumentedTest` | 第 47-54 行：点击 `smart_custom_en`、`smart_youtube` 后改为在详情页断言。 |
| `LinkFeedbackTest`、`SmartDragTest`、`AppFeatureTest` | `smart_apps_*` 改为在详情页查找。 |
| `HomeGroupsTest` | 确认 `smart_target_youtube` 点击仍打开选择器。 |
| 新增断言 | 1.0 倍字号时，`smart_card_speed` 高度不超过 64 dp（用 `getBoundsInRoot`）。2.0 倍字号时，`smart_latency_speed` 完整显示。 |

测试必须测试真实实现。不得把业务逻辑复制进测试。

## 7. 验收（全部在 S25+ 模拟器）

规则（来自项目 `AGENTS.md`）：

- 每次只运行一台模拟器。不得使用快照。
- 所有模拟器命令都用 `tests/with_s25_emulator.sh COMMAND [ARGS...]` 包裹。
- 每次结束后，输出中必须出现 `S25_CLEANUP_PASS`。
- S25+ 模拟器只代表屏幕布局，不代表三星 One UI 或真机耗电。

验收项，必须全部通过：

1. Rust 单元测试：`cargo test --manifest-path rust/locus_core/Cargo.toml` 全部通过。
2. 构建与 JVM 测试：`./gradlew :app:testDebugUnitTest --rerun-tasks :app:assembleDebug :app:assembleDebugAndroidTest :app:assembleRelease` 全部通过。release 构建包含 `:app:verifyReleaseSigning`。
3. 默认回归组：`tests/with_s25_emulator.sh tests/run_emulator_acceptance.sh`，使用脚本默认的 15 个类，`result.json` 为 PASS。
4. 智能分流组：`ZANE_TEST_CLASSES` 设为下列 18 个类，用同一脚本运行，`result.json` 为 PASS：
   `SmartDragTest`、`SmartTargetPickerTest`、`SmartDomainPickerTest`、`SmartRuleCommitInstrumentedTest`、`SmartOrderRuntimeTest`、`LinkFeedbackTest`、`TargetLatencyUiTest`、`TargetLatencyRuntimeTest`、`FunctionFeedbackTest`、`UiLanguageRepairInstrumentedTest`、`RuleKeyboardLayoutTest`、`AppFeatureTest`、`HomeGroupsTest`、`HomeInteractionTest`、`FloatingToolbarTest`、`UiSkinTest`、`FontScaleTest`、`TouchTargetTest`（每个都加包名前缀 `com.zane.zanebox.`）。
5. 截图（`adb exec-out screencap -p` 直接存到电脑）：
   - 智能分流主页和策略详情子页，5 套皮肤（`linear`、`claude`、`supabase`、`miro`、`prism`）各一张。
   - 1.0 倍和 2.0 倍字号各一张。
   - 与节点页截图并排对比：策略卡与节点卡的高度、字号、图标列对齐、延迟颜色一致。
6. 滑动帧率（release APK，240 节点）：
   - 数据使用 1.0.3 的同一夹具 `reports/s25-emulator-20261124-074855.pKoQWj/zanebox-realistic-240.db`。
   - 指标与 `repro-realistic-summary.json` 相同（`dumpsys gfxinfo`，智能分流页上下各滑 2 次）。
   - 未确认：1.0.3 的帧率测量命令没有提交到仓库。可以参考 `tests/measure_scroll.py`，但该脚本的包名是旧的 `com.zane.zanebox`，当前 `applicationId` 是 `com.zane.locus`（`app/build.gradle.kts:34`），必须先修正。测量脚本必须提交到 `tests/`。
   - 门槛：智能分流页 gfx 95th ≤ 18 ms，99th ≤ 20 ms，`Number Missed Vsync` = 0，`Number Slow UI thread` = 0。1.0.3 基线为 95th 17 ms、99th 18 ms。
7. 手动走查（模拟器，可用 Maestro MCP）：
   - 点延迟：显示「测试中」，结束后显示数值和颜色；目标为分组时，第二行节点名切换为最低延迟节点。
   - 点第二行：打开分流目标选择器；选择后第二行更新。
   - 点卡片其他区域：打开详情子页。
   - 在 URL 对话框和域名对话框中点「确定」，返回主页再进入，修改仍在。
   - 长按拖动排序仍然有效，拖动时卡片高度不变。
   - 页头「立即更新」有反馈；菜单中两项更新设置可用；菜单「节点汇总组」可进入。
8. 证据：所有报告保存在 `reports/s25-emulator-*` 下，汇总写入一个 `acceptance-1.0.4.json`，逐项写 PASS 或 FAIL、命令、关键输出。

任何一项 FAIL：修复后重跑受影响的整组。不得降低门槛。不得改版本号，不得提交，不得发布。

## 8. 版本、提交与发布（第 7 节全部 PASS 后才执行）

1. `version.properties` 改为 `versionName=1.0.4`、`versionCode=5`。
2. 用新版本号重新执行第 7 节第 2 项构建，并重跑第 7 节第 4 项（智能分流组）确认版本号变化未引入问题。
3. 生成 `app/build/outputs/apk/release/Locus-1.0.4-arm64-v8a.apk` 和同目录 `.sha256`。验证签名证书 SHA-256 为 `501a5ab904e2e601d38cf8b04b19cc3165a503235710d888ff121d2c55d9ee29`（`docs/release-signing.md`）。
4. 提交前检查 `git status` 和 `git diff --stat`。只提交本任务的源码、测试、脚本、文档和 `version.properties`。不得提交 `build/`、`.build/`、`reports/` 下的大文件、签名文件或任何凭据。
5. 提交信息参照 `e1fc2fa` 的格式，例如：`feat: redesign Smart Routing UI to match node page and release Locus 1.0.4`，正文逐条写改动和验收数据。
6. 打标签 `v1.0.4`，推送 `main` 和标签到 `origin`（`https://github.com/zane0314/locus.git`）。
7. 创建 GitHub Release `v1.0.4`。标题和说明格式参照 `v1.0.3` 的 Release。附件只放 `Locus-1.0.4-arm64-v8a.apk` 和 `.sha256`。不得上传 JKS、`signing.properties` 或密码。
8. Git 作者身份使用本机现有配置，不得修改。

## 9. 交付汇报

完成后向用户汇报：

- 第 7 节每一项的状态、命令和关键输出。
- 策略卡改版前后的高度（dp）。
- 帧率前后对比（95th、99th、Missed Vsync、Slow UI thread）。
- 提交号、标签、Release 链接、APK SHA-256、签名证书 SHA-256。
- 未做的事和原因。
