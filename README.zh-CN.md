# Flare 共享协议

[English](README.md)

`proto/` 维护 `flare.common.v1` 的共享 Protobuf 定义，覆盖消息、会话、同步、事件和推送信封。协议演进需保持文件名、包名和字段编号与已发布的客户端和服务端兼容。

本地运行 `buf build` 校验编译。CI 通过 `buf breaking` 与上一版协议对比。共享约定见[协议设计](docs/im-contract-design.md)。

- [Rust 绑定和 Core gRPC](https://github.com/flare-im/flare-protocol-grpc) 发布 `flare-proto` 和 `flare-grpc-proto`。
- [插件 DevKit](https://github.com/flare-im/flare-plugin-devkit) 维护插件 Manifest 和 Rust 校验器。
- [通知规则](https://github.com/flare-im/flare-notify-rules) 维护客户端和推送服务共用的通知决策。

协议源码使用 [Apache-2.0](LICENSE) 许可证。
