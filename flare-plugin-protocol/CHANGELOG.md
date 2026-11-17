# 变更记录

`flare-plugin-protocol` 的变更。UI 词汇版本（`ui::UI_VOCABULARY_VERSION`）每加一批词汇加 1，
新增的集合在这里登记；选择变体只看集合，不看版本号。

## 未发布

### Web 包（L3）

- `bundle`：`WebBundleDecl`（Manifest `clientExtensions.webBundles`，`id`、`sha256`（小写十六进制、无前缀）、`size`、
  `bridge`、`digitalGoods`；目录另带 `urls`、`files`、已批准的桥权限 `permissions`）；`BundleManifest`（`bundle.json`）
  与 `validate_for`；路径与类型白名单、D-05 上限；入口 HTML 静态规则 `check_entry_html`；脚本扫描 `scan_script`。
  特性 `bundle-archive` 下：`check_archive`（平台回读）、`unpack_archive`（客户端缓存）、`write_archive`（CLI 打包）。
- `bridge`：`BRIDGE_VERSION = 1`、`BridgeEnvelope`、`BridgeMethod`、`BridgeRequest` 与各方法参数结果、事件与载荷、
  固定错误码 `BridgeErrorCode`、`BRIDGE_PERMISSIONS`、`BridgeGrant`（`bundle.json` ∩ 租户批准 ∩ 目录授权）。
- `runtime`：投影里每个插件带 `webBundles`（`ProjectedWebBundle`：`id`、`sha256`、本应用镜像 `urls`、解包副本 `files`、
  本应用批准的桥权限 `permissions`），校验数量、地址与摘要；`ResolvedPlugin.web_bundles` 是 Core 解析后可下发的目录形态，
  经 `with_web_bundles` 设置，必须与 Manifest 声明一致。
- `UiContribution::select_with_bundles`：只选可用的包、按声明的桥版本比对、数字商品不选给受 App Store 规则约束的客户端。
- Manifest 校验：变体打开的包必须声明，声明的包必须被某个变体打开，至多 8 个。
- 生成器 `plugin-bridge-schema` 写出 `plugin_web_bundle.schema.json` 与 `plugin_bridge.schema.json`。
- `MediaReadResult.url` 是宿主给的 `data:` 地址（≤4 MiB），插件 Web SDK 转成 `blob:` 地址交给包。

### 客户端读取

- `json::from_value`：客户端读 JSON 值的唯一入口，与 `serde_json::from_value` 一致，只是结构体只从对象读，
  按字段顺序写成的数组被拒绝（`invalid type: sequence, expected struct …`）。`kind` 标签枚举的变体经它读取。
- `ui::ShownPluginView`：宿主传回的视图（`previous`）按 JSON 保留，翻页只追加条目与行，结果与
  `ResolvedPluginView::append_page` 相同；客户端不再把解析后的视图读回类型。

### UI 词汇版本 2

在版本 1（`ui::base_vocabulary()`）之上新增：

- 块：`section`、`stats`、`chart`、`image`、`qr`、`hero`、`steps`、`banner`、`timeline`。
  `section` 只嵌一层，子块计入文档 24 块上限；`image` 每文档至多 4 个，`hero` 至多 1 个，
  `banner` 至多 2 个。
- 输入：`money`（值为 `{amount, currency}`，金额是十进制字符串）、`image`、`document`（带 `side`）、
  `location`（带 `precision`，值为 `ui::UiLocationValue` `{latitude, longitude, accuracyM, precision}`，
  经纬度位数与精度下限按 `precision` 校验）。
- 格式：`percent`、`duration`、`count`、`bytes`。
- 字段：`entries.timePath`（条目时间）、`rows.groupByDay`（按首个 `time` 列分日标题）、
  `hero.maskable`（查看者可隐藏数值）。
- 原生能力：`paymentConfirmation`、`documentCapture`、`locationOnce`、`rtcSession`；
  原生动作目标增加 `params`（每个能力固定结构）和 `into`（结果写入同文档的输入）。
  `documentCapture` 与 `locationOnce` 必须带 `into`：前者写同 `side` 的 `document` 输入，
  后者写同 `precision` 的 `location` 输入。

### 新增

- 操作声明 `transient`（只用于 `command`）：只能在宿主向查看者确认后调用（支付确认订单的 `operation`），
  声明的动作不能指向它；客户端目录 `ClientPlugin.transientOperations` 列出已授权的这类操作。
- 客户端 UI 能力描述 `ui::ClientUiDescriptor` 与词汇 `ui::UiVocabulary`：`engine_vocabulary()`
  由枚举生成，`UiVocabulary::for_host` 取引擎与 Kit 词汇的交集，`native` 再与宿主允许名单取交集；
  `ClientUiDescriptor::hash` 是规范化后的 SHA-256。
- 贡献变体：`UiContribution.variants`、`minClient`，目录专用的 `variant`、`upgrade`、`bundle`；
  `UiContribution::select` 选出第一个满足描述的变体，或给出升级占位；
  `UiContributionVariant::missing_requires` 供发布检查，`requires` 只需写出版本 1 之外的部分。
- `UiUnavailableReason::UpgradeRequired` 与 `ResolvedPluginView.minBuild`。
- 宽容解析入口 `ui::lenient::prepare_contribution`；每个块可带 `required`。
- 操作描述 `ownerScoped`：用户页面上的 `protected-media` 只经本人范围的 `query` 读取。
- `ui::PaymentConfirmationOrder`：操作结果 `data.confirmation` 的待确认订单。

### 变更

- `UiActionTarget::Native` 增加字段 `params`、`into`，按结构体字面量构造的代码要补上
  （或改用 `UiActionTarget::native`）。
- 对 `UiBlock`、`ResolvedUiBlock`、`UiFieldFormat`、`ResolvedUiFormat`、`UiInputKind`、
  `NativeCapability`、`UiUnavailableReason` 做穷尽匹配的下游代码需要补上新分支。
- 管理页（`admin.pages`）文档里的动作除了调用管理操作，还可以打开本插件的另一个管理页：
  `{"kind":"page","page":"<pageId>"}`，不带 `pluginId`，不是自己，不带 `form`、`confirm`、`command`、
  `continuation`，`input` 的每个键都是目标页的筛选输入；详情文档的动作仍只能调用管理操作。
  `contractVersion` 仍是 1；用了这种动作的清单要用 `hostVersionRange` 要求带这项改动的平台。
