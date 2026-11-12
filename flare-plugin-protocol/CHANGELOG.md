# 变更记录

`flare-plugin-protocol` 的变更。UI 词汇版本（`ui::UI_VOCABULARY_VERSION`）每加一批词汇加 1，
新增的集合在这里登记；选择变体只看集合，不看版本号。

## 未发布

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
