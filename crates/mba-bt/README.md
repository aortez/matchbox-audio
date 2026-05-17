# Matchbox Bluetooth Request Router

`mba-bt` is the local-first Bluetooth control daemon crate. Phase 2 keeps the
core request handling independent from BlueZ and Android so it can be tested
without Bluetooth hardware.

Current pieces:

- `RequestRouter` accepts complete `ProtocolMessage` requests and returns
  protocol responses/events.
- `FakePlayerBackend` supplies deterministic `system.snapshot` responses for
  tests and the local exerciser.
- `SessionGate` enforces one active client and returns structured `busy`
  errors for later connection code.
- `FrameDecoder` and `encode_frame` provide a local length-prefixed development
  transport.
- `InMemoryBleTransport` proves BLE chunks can reassemble into the same router
  path used by framed local transports.

Run the local stdio exerciser:

```sh
cargo run -p mba-bt -- --stdio
```

The stdio mode reads and writes the same local frame format documented in the
Android app plan:

```text
uint32_le json_length
utf8_json_payload
```

This is intended for small developer tools and tests. Real BLE GATT
registration remains Phase 3.
