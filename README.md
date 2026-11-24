# Flare Protocol

[简体中文](README.zh-CN.md)

The `proto/` directory is the source of truth for `flare.common.v1` messages, including messages, conversations, sync, events and push envelopes. Keep file names, package names and field numbers compatible with released clients and servers.

Run `buf build` to compile the schemas. CI also runs `buf breaking` against the previous schema. The [protocol design](docs/im-contract-design.md) describes the shared wire contracts.

- [Rust bindings and Core gRPC](https://github.com/flare-im/flare-protocol-grpc) publish the `flare-proto` and `flare-grpc-proto` crates.
- [Flare Plugin DevKit](https://github.com/flare-im/flare-plugin-devkit) owns plugin manifests and their Rust validators.
- [Flare Notification Rules](https://github.com/flare-im/flare-notify-rules) owns the shared client and push-server decision logic.

The source is licensed under [Apache-2.0](LICENSE).
