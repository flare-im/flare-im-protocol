# 变更记录

`flare-plugin-protocol` 的变更。UI 词汇版本（`ui::UI_VOCABULARY_VERSION`）每加一批词汇加 1，
新增的集合在这里登记；选择变体只看集合，不看版本号。

## 未发布

### UI 词汇版本 2

在版本 1（`ui::base_vocabulary()`）之上新增：

- 块：`section`、`stats`、`chart`、`image`、`qr`、`hero`、`steps`、`banner`、`timeline`。
  `section` 只嵌一层，子块计入文档 24 块上限；`image` 每文档至多 4 个，`hero` 至多 1 个，
  `banner` 至多 2 个。
- 输入：`money`（值为 `{amount, currency}`，金额是十进制字符串）、`image`、`document`（带 `side`）。
- 格式：`percent`、`duration`、`count`、`bytes`。
- 原生能力：`paymentConfirmation`、`documentCapture`、`locationOnce`、`rtcSession`；
  原生动作目标增加 `params`（每个能力固定结构）和 `into`（结果写入同文档的输入）。

### 新增

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
