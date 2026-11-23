# Design System: Locus

## 1. Visual Theme & Atmosphere

Locus is a high-precision Android proxy workbench built on Jetpack Compose and Material 3. Its default visual identity (**深夜线条 / Linear Dark**) adopts Linear's obsidian-dark engineering aesthetic: near-black canvas (`#010102`), elevated dark slate cards (`#0F1011` / `#18191A`), crisp `1dp` / `0.5dp` hairline borders (`#23252A`), cool mist-white typography (`#F7F8F8`), indigo-violet accent (`#5E6AD2` / `#828FFF`), and tabular monospace telemetry for latencies, ports, and protocol badges.

Rather than relying on heavy drop shadows or decorative gradients, Locus communicates hierarchy through **luminance stepping**, **1px structural borders**, and **functional status color**:
- **Primary Default (`linear` — 深夜线条)**: Obsidian precision dark mode (`#010102` canvas, `#5E6AD2` indigo brand accent).
- **Warm Editorial (`claude` — 暖纸陶色)**: Warm parchment light mode (`#FAF9F5` canvas, `#F5F0E8` surface, `#CC785C` terracotta accent, serif page display titles).
- **Developer Emerald (`supabase` — 深色翡翠)**: Studio charcoal dark mode (`#1C1C1C` canvas, `#252525` surface, `#3ECF8E` signal emerald with `#171717` high-contrast dark ink on primary actions).
- **Workshop Canvas (`miro` — 明黄工作台)**: Crisp daylight canvas (`#FFFFFF` background, `#F7F8FA` neutral cards, `#1C1C1E` ink primary, `#FFD02F` workshop yellow highlights reserved for selected/active badges).
- ** Heritage Glass (`prism` — 棱镜流光)**: Retained classic translucent light theme with floating pill toolbar (`ZaneHomeToolbar`) for visual continuity.

---

## 2. Color Palette & Semantic Roles

Every skin maps into `ZaneSkinPalette` and Material 3 `ColorScheme` (including `inverseSurface`, `inverseOnSurface`, and `inversePrimary` so Snackbars and dialogs never invert into jarring white blocks on dark skins).

### Core Palette (`linear` — 深夜线条)
- **Canvas Background (`background`)**: `#010102`
- **Card / List Surface (`surface`)**: `#0F1011`
- **Elevated Control / Hover Surface (`surfaceElevated`)**: `#18191A`
- **Hairline Border (`hairline` / `divider`)**: `#23252A`
- **Strong Border (`hairlineStrong`)**: `#343841`
- **Primary Brand Accent (`primary`)**: `#5E6AD2`
- **Interactive Accent (`primaryDark`)**: `#828FFF`
- **Selected Row Surface (`selectedSurface`)**: `#151828`
- **Selected Chip Surface (`chipSelectedBg` / `iconTile`)**: `#1B1E36`
- **Primary Text (`onSurface`)**: `#F7F8F8`
- **Secondary Metadata (`secondaryText`)**: `#8A8F98`
- **Tertiary / Muted Text (`textTertiary`)**: `#62666D`
- **Latency & Connection Status**:
  - Fast / Connected (`success`): `#27A644` (`1–150 ms`)
  - Medium (`warning`): `#F5A623` (`151–350 ms`)
  - Slow / Error (`error`): `#E5484D` (`>350 ms` or failed at `80%` alpha)

### Alternate Skins
| Role | `claude` (暖纸陶色) | `supabase` (深色翡翠) | `miro` (明黄工作台) | `prism` (棱镜流光) |
|---|---|---|---|---|
| `background` | `#FAF9F5` | `#1C1C1C` | `#FFFFFF` | `#EAEBF3` |
| `surface` | `#F5F2EA` | `#252525` | `#F7F8FA` | `#66FFFFFF` |
| `surfaceElevated` | `#EDE7DC` | `#303030` | `#EFF1F5` | `#F7F8FA` |
| `primary` | `#CC785C` | `#3ECF8E` | `#1C1C1E` | `#176D95` |
| `primaryDark` | `#A9583E` | `#24B47E` | `#1C1C1E` | `#176D95` |
| `divider` | `#E5DEC9` | `#383838` | `#E0E2E8` | `#E8EAED` |
| `selectedSurface` | `#EFE6D8` | `#193D2C` | `#FFF4C4` | `#14229ED9` |
| `iconTile` | `#EFE6D8` | `#193D2C` | `#FFD02F` | `#E5F4FB` |

> **Pixel-Fingerprint Invariant**: All 5 skins maintain pairwise distinct outer-margin `background` colors (`< 97%` pixel match) on Settings pages for deterministic visual verification.

---

## 3. Typography Rules

Locus uses a disciplined **4-tier type scale** (`20sp` / `17sp` / `14sp` / `12sp`) with weights capped at **SemiBold (600)**, **Medium (500)**, and **Regular (400)** (never heavy `700+` Bold):

| Role | Size / Line Height | Weight | Letter Spacing | Notes |
|---|---|---|---|---|
| **Page / Brand Title** | `20sp` / `26sp` | `SemiBold (600)` | `-0.3sp` | Serif on `claude`; Sans-serif elsewhere. Aligned cleanly with navigation icon without extra left offset. |
| **Connection Card Hero** | `17sp` / `22sp` | `SemiBold (600)` | `-0.2sp` | Single-line ellipsis (`maxLines = 1`) so card height never jumps across nodes. |
| **Row Title (Node / Setting / Policy)** | `14sp` / `20sp` | `Medium (500)` (`SemiBold` when selected) | `-0.1sp` | Unified across Home, Smart Routing, and Settings Hub. |
| **Metadata / Group Chip / Latency / Caption** | `12sp` / `16sp` | `Medium (500)` or `Regular (400)` | `0sp` | Monospace (`FontFamily.Monospace`) for latency values, protocol tags, core names, and ports. |

---

## 4. Component Stylings

1. **Top Action Bar (`HomeHeader`)**:
   - Brand mark + `Locus` wordmark on the left for structured skins.
   - Action buttons (`search_toggle`, `add_nodes`, `node_menu`) preserve full `48dp × 48dp` touch targets while rendering compact `38dp × 38dp` `8dp`-rounded elevated containers (`1dp` hairline border on dark skins) instead of oversized `48dp` solid circles.
2. **Connection Workbench Card (`WhiteConnectionCard`)**:
   - Compact 3-row structure:
     1) Status indicator dot + status text (`connection_status`) on left; monospace protocol badge on right.
     2) Selected node title (`home_selected_node`, `17sp`, `maxLines = 1`, locale-aware separator).
     3) Mode summary + compact kernel selector pill (`core_switcher` -> `sing-box ▾` dropdown with `core_chip_sing-box`, `core_chip_xray`, `core_chip_mihomo`) on left; primary/secondary action button (`connect_toggle`, `40dp` visual height inside `48dp` touch target) on right.
   - **Action Button States**: Disconnected uses filled `primary` (`连接`); Connected uses neutral elevated surface with `1dp` hairline border (`断开`) while the card border and status dot carry semantic `success` green.
3. **Group Filter Strip (`HomeGroupChip`)**:
   - `36dp` visual pill (`6dp` radius on `linear`/`supabase`, `10dp` on `claude`, pill on `miro`) centered in an accessible touch target; selected chip uses `chipSelectedBg` + `primary` border. No redundant full-width divider directly underneath.
4. **Node List & Auto-Select Row**:
   - `自动选择` (`home_auto`) places its `RadioButton` on the **left**, aligned on the exact same vertical axis as node rows.
   - Node rows (`minHeight = 60dp`) use crisp `1dp` / `0.5dp` cards with a left accent indicator when selected, 3-tier semantic latency colors across all skins, and compact secondary action icons (`node_info_<id>`, `node_menu_<id>`).
5. **Compose-Native Toggle (`UiSwitch`)**:
   - Pure Compose M3-styled track (`40dp × 22dp`) and animated thumb (`16dp`) centered inside an exact `48dp × 36dp` accessible `Role.Switch` container—eliminating legacy Holo/Material1 `android.widget.Switch` artifacts.
6. **Bottom Navigation (`WhiteNavigation`)**:
   - `56dp` compact bar above `navigationBarsPadding()` with `1dp` top hairline divider and subtle pill highlight behind the active icon.

---

## 5. Layout, Motion & Test-Clock Discipline

- **Page Padding**: `16dp` horizontal content margins; `8dp` vertical rhythm between list items; `24dp` bottom scroll clearance so the final item (`factory_reset`) clears `subpage_list` at both `1.0x` and `2.0x` font scales.
- **Deterministic Finite Motion**: State transitions (connection status color, switch thumb offset, card border) use `180ms` finite `tween` animations. Infinite loop animations (`rememberInfiniteTransition`) are strictly forbidden so Jetpack Compose `waitForIdle()` test synchronization never blocks.
