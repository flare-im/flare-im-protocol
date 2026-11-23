# 变更记录

`flare-plugin-protocol` 的变更。UI 词汇版本（`ui::UI_VOCABULARY_VERSION`）每加一批词汇加 1，
新增的集合在这里登记；选择变体只看集合，不看版本号。

## 未发布

### 插件体验交接（K1，都是可选字段，旧清单与旧订单解析不变）

- `UiDocument.errors`：文档按插件的拒绝码（`[A-Z][A-Z0-9_]*`，≤64 字节，≤32 个）声明给查看者的话，经 `texts`
  翻译；`UiDocument::refusal_text(code, locale)`；`is_refusal_code`。引擎把拒绝结果的 `errorCode` 换成这句话交给宿主。
- `UiInput.help`：字段下方的说明（经 `texts` 翻译，同占位的长度限制），占位只写示例（R6）。
- `UiPageTransition.run`：落地页加载后宿主执行一次的动作 id（只在该次导航的首次加载，且是无表单、无确认、未禁用的
  operation 动作）；与 `actionId` 互斥；导航结果顶层带 `run`。清单校验：目标页在本插件时必须声明该动作。
- `PaymentConfirmationOrder.requestedBy`：付款所为的插件 id（如钱包订单替红包付款），宿主「由 X 发起」用它。
- `UiMoneyFormat.currencyPath` → `ResolvedUiMoney.currency`：金额的币种代码（≤32 个字母数字或 `_.-`，不合格的不带）。
- 金额（`money`）的最小单位值可带显式 `+`（记账行的收入，R10），解析后的文字与 `ResolvedUiMoney.minor` 保留它；只有
  符号没有数字的值照旧按原文显示。

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

### UI 词汇版本 3

在版本 2 之上新增（插件体验，设计见 flare-im-design `docs/plugin-experience.md`）：

- 块：`result`（`statePath` + `values`（`1..=32` 个 `{label, tone}`）+ 可选 `amountPath`、`format`、
  `descriptionPath`），每文档至多 1 个；解析为 `{state, title, tone, unmapped?, amount?, format?, description?}`。
- 字段：`fields[].values` 与 `rows.columns[].values` 的取值可写 `{label, tone}`（`ui::UiValueLabel`，字符串形态照旧）；
  动作可选 `group`（`common` | `records`）与 `icon`（kit 图标名）。三者都只管呈现。
- 解析结果：字段与单元格带 `tone`（取值声明了色调时）；取值不在插件声明的枚举里时带 `unmapped: true`
  （文本仍是原值，供旧 kit 显示；新 kit 显示「未知状态」）。字段与列的取值表含带色调的取值才算枚举，纯文字表
  仍可只给自由文本的部分取值起名，其余照原样显示；`status` 块、徽标、时间线状态与 `result` 有取值表即是枚举。
- 动作可选 `disabledWhen`（`0..=4` 个条件，读法同 `hiddenWhen`，`hiddenWhen` 先判）与 `disabledReason`
  （二者同有同无；原因按界面文字校验，经 `texts` 翻译）。解析后的动作只在条件成立时带 `disabledReason`，
  kit 见到它即画禁用键并在键下写原因（设计 R2.5）；只管呈现，插件仍自行拒绝调用。`rows` 块拥有的行动作不得声明。
- 付款确认订单 `PaymentConfirmationOrder` 可选 `shortfall`（差额，十进制字符串，校验同 `amount`，须大于 0）与
  `topUp`（`page` 目标，可带 `pluginId`）：宿主据此画余额不足态（确认键禁用、写差额、「去充值」打开 `topUp`，设计 4.3）。
  引擎按记录的订单原样交给宿主的 `confirmPayment` 请求。只认旧字段的引擎（`deny_unknown_fields`）读不出带这两个字段的订单。
- 页签：`page` 贡献可选 `tabs`（`manifest::UiContribution::tabs`，`2..=4` 个 `ui::UiPageTab {page, label}`），
  列出本插件的同级页面（本页在内，每页声明同一组页签，标签经 `texts` 翻译）；解析后的页面视图带
  `tabs: [{page, label, selected}]`（`ui::ResolvedUiPageTab`），kit 在页头下画分段控件，选中即以该页替换当前页
  （不新增返回层级，参数照旧）。只管呈现，不跨变体改变。宿主传回的已显示视图可带 `tabs`。
- 记录行图标：`rows` 块可选 `icon`（kit 图标名，校验同动作 `icon`），解析结果原样带出；kit 在页面列表每行前画
  36 圆底图标，并按记录行（同 5.8 账单行）排版：第一列为行尾的值、带色调的列为值下方的标签，第二列为标题，其余列为标题下一行（设计 R2.3）。
- 旧清单原样有效：没有新字段的文档解析结果与之前相同，只多出缺省为假的 `unmapped`（不序列化）。

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
- 拒绝说明 `rejection`：明确拒绝的 `error_message` 可以是 JSON 对象，键是语言（`^[a-z]{2,3}(-[A-Z]{2})?$`，
  `1..=4` 个，`MAX_MESSAGE_LOCALES`），值是 `1..=300` 字（`MAX_MESSAGE_CHARS`）、没有控制字符、格式字符
  （bidi 嵌入/覆盖/隔离符、零宽字符、字节序标记、标签字符，与审计原因同一组）与行段分隔符的文字；整个对象至多
  1024 字节（`MAX_MESSAGE_BYTES`，Core 只转发这么长的说明，两种语言各 300 个汉字就已超过）；
  其他形状一律视为没有说明。`parse_message` 校验形状，`message_for` 先取请求的语言、再取 `zh-CN`。
  只由读的一方校验；不改清单与票据结构。

### 变更

- `UiActionTarget::Native` 增加字段 `params`、`into`，按结构体字面量构造的代码要补上
  （或改用 `UiActionTarget::native`）。
- 对 `UiBlock`、`ResolvedUiBlock`、`UiFieldFormat`、`ResolvedUiFormat`、`UiInputKind`、
  `NativeCapability`、`UiUnavailableReason` 做穷尽匹配的下游代码需要补上新分支。
- 管理页（`admin.pages`）文档里的动作除了调用管理操作，还可以打开本插件的另一个管理页：
  `{"kind":"page","page":"<pageId>"}`，不带 `pluginId`，不是自己，不带 `form`、`confirm`、`command`、
  `continuation`，`input` 的每个键都是目标页的筛选输入（目标页没有行块就不读数据、没有筛选，`input` 只能为空）；
  详情文档的动作仍只能调用管理操作。
  `contractVersion` 仍是 1；用了这种动作的清单要用 `hostVersionRange` 要求带这项改动的平台。
- 管理页（`admin.pages`）的行块最多 8 个行动作（`ui::MAX_ADMIN_UI_ROW_ACTIONS`，控制台在行上显示前几个，其余收进「更多」）；
  客户端的贡献页与卡片仍是 4 个（`ui::MAX_UI_ROW_ACTIONS`）。`UiDocument::validate` 按客户端上限检查，
  管理页文档用 `UiDocument::validate_within(UiDocumentLimits::ADMIN)`。随宿主 1.1.0 发布：
  行动作超过 4 个的清单要用 `hostVersionRange` 要求 1.1.0 起的平台；`contractVersion` 仍是 1。
- 行块里每个行动作多了 `values`（`ResolvedUiRowAction::values`，为空时不出现）：行动作的表单从这一行开始，
  按输入 id 给出该行在输入的 `defaultPath` 处、符合页面解析后输入的值；行里没有的输入仍从按页面数据解析的
  `default` 开始，所以行表单的路径要和页面自己的字段分开命名。按结构体字面量构造 `ResolvedUiRowAction` 的代码要补上
  `values`。随宿主 1.1.0 发布；旧宿主不带这个字段，表单照旧空白打开。
- 管理页解析后的页面动作（`actions` 与 `menuActions`）多了 `values`（`UiAction::values`，为空时不出现，
  只由 `admin::AdminPage::resolve` 给出）：按输入名给出页面数据在 `input` 各路径处的值，路径不存在时为 null；
  控制台没有页面数据，把它原样作为动作的 `input` 发回。平台把 null 当作未传交给操作，除非操作入参 schema 根上的
  `required` 列出了它（那时拒绝）；缺名字的入参照旧拒绝。清单里声明 `values` 的动作被拒绝，解析时也丢掉声明里的
  `values`，只有 `AdminPage::resolve` 给出。按结构体字面量构造
  `UiAction` 的代码要补上 `values`。随宿主 1.1.0 发布；旧宿主上绑定页面数据的页面动作仍被拒绝。
- 行动作的入参按同一规则交给操作：行里没有的输入（null）当作未传，除非操作入参 schema 根上的 `required` 列出了它；
  以前平台一律拒绝带 null 的行动作。
- 声明了 `choicesPath` 的下拉输入（不论有没有列出兜底的 `choices`）在按声明的文档校验表单时（没有页面数据，
  如平台的管理页）只按形状检查值：非空、单行、至多 128 字节，不再只认兜底选项。按页面数据解析后的输入仍只认读到的
  选项，声明的 `default` 仍须是列出的选项之一。宿主不再核对这类值是否在选项里，插件必须自己再校验。
