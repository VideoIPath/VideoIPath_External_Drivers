# VideoIPath External Driver external

A Python reference implementation of a VideoIPath External Driver using WebSockets.

The external connects to a VideoIPath server, exposes a configurable device structure, responds to protocol requests, stores dynamic values in memory, validates incoming value updates, and automatically reconnects after a disconnection.

## Features

- WebSocket connection to a VideoIPath External Driver endpoint
- Bearer-token authentication
- Support for `ws://` and `wss://` endpoints
- Optional TLS certificate verification bypass for test environments
- Automatic reconnection every five seconds
- Device structure discovery with `get_structure`
- Dynamic value reads with `get_dynamic_values`
- Dynamic value updates with `set_dynamic_values`
- Numeric, boolean, and enumerated string values
- GPI and GPO capability metadata
- Keepalive responses
- Empty alarm-list responses
- Logging of received messages, transmitted messages, and value changes

## Requirements

- Python 3.10 or later
- The `websockets` Python package

Install the dependency:

```bash
python -m pip install websockets
```

## Running the external

Assuming the source file is named `vip_external_driver_example.py`:

```bash
python vip_external_driver_example.py \
  --url wss://vip.example.com/x-driver/v1 \
  --token "<access-token>" \
  --node-id 550e8400-e29b-41d4-a716-446655440000
```

The program appends the node ID to the URL as a query parameter:

```text
?node_id=<node-id>
```

Use `--insecure` only in a trusted development or laboratory environment with a self-signed or otherwise untrusted certificate:

```bash
python vip_external_driver_example.py \
  --url wss://vip.example.com/x-driver/v1 \
  --token "<access-token>" \
  --node-id 550e8400-e29b-41d4-a716-446655440000 \
  --insecure
```

> **Security warning:** `--insecure` disables TLS certificate and hostname verification. Do not use it in production.

## Command-Line Arguments

| Argument | Required | Description |
|---|---:|---|
| `--url` | Yes | WebSocket endpoint, for example `wss://vip.example.com/x-driver/v1` |
| `--token` | Yes | Bearer token used in the `Authorization` header |
| `--node-id` | Yes | Driver node ID. It must be a valid UUID. |
| `--insecure` | No | Disables TLS certificate verification for `wss://` connections |

## Device Structure

The external reports one module named `module-1`.

### Senders

| ID | Label | Format |
|---|---|---|
| `sender-video` | Sender video | Video |
| `sender-audio` | Sender audio | Audio |

### Receivers

| ID | Label | Format in the supplied code |
|---|---|---|
| `receiver-video` | Receiver video | Video |
| `receiver-audio` | Receiver audio | Video |

> **Note:** The supplied code declares `receiver-audio` with `format_type: "Video"`. Change it to `"Audio"` if this receiver is intended to carry audio.

### Network Interface

| ID | Label | Type |
|---|---|---|
| `eth-1` | IP Interface | Single |

All senders and receivers reference `module-1` and `eth-1` in their interface options.

## Dynamic Values

Dynamic values are defined in the `PARAMS` dictionary. The device structure and initial runtime values are generated from this table.

| ID | Data type | Default | Access metadata | GPI | GPO |
|---|---|---:|---|---:|---:|
| `width` | Number | `1920` | `read_write` | No | No |
| `height` | Number | `1080` | `read_write` | No | No |
| `opacity` | Number | `100` | `read_write` | No | No |
| `gpi` | Boolean | `false` | `read_only` | Yes | No |
| `gpo` | Boolean | `false` | `write_only` | No | Yes |
| `gpio` | Boolean | `false` | `read_write` | Yes | Yes |
| `text_select` | String enumeration | `VideoIPath` | `read_write` | No | No |

The `access`, `gpi_capable`, and `gpo_capable` fields are currently exposed as metadata. The supplied `dispatch()` implementation does not enforce read-only or write-only access when processing `get_dynamic_values` and `set_dynamic_values`.

## Supported Value Types

### Number

Numbers are represented as integers. The external validates the configured minimum and maximum values.

Example parameter:

```python
"width": {
    "label": "Width",
    "description": "Output width",
    "min": 0,
    "max": 1920,
    "step": 1,
    "unit": "px",
    "default": 1920,
    "access": "read_write",
    "gpi_capable": False,
    "gpo_capable": False,
}
```

The current validation checks the integer type and configured range. It publishes the configured `step` in the structure but does not enforce step increments.

### Boolean

Boolean parameters accept JSON booleans and the integers `0` and `1`:

```json
{
  "gpio": true
}
```

```json
{
  "gpio": 1
}
```

### String Enumeration

A string parameter can optionally define an allowed `values` list:

```python
"text_select": {
    "type": "string",
    "label": "Product",
    "description": "Which product to display",
    "values": ["VideoIPath", "Virtuoso"],
    "default": "VideoIPath",
    "access": "read_write",
    "gpi_capable": False,
    "gpo_capable": False,
}
```

> **Important:** The supplied code contains `"VidoeIPath"` in the allowed values but uses `"VideoIPath"` as the default. Correct the spelling in `values` as shown above so the default is also a valid selectable value.

## Protocol Messages

Every response reuses the `message_id` from the corresponding request.

### Keepalive

Request:

```json
{
  "message_id": "example-1",
  "message_type": "keepalive",
  "data": null
}
```

Response:

```json
{
  "message_id": "example-1",
  "message_type": "keepalive",
  "data": null
}
```

### Get Structure

Request:

```json
{
  "message_id": "example-2",
  "message_type": "get_structure",
  "data": null
}
```

The response contains the complete device structure in `data`.

### Get All Dynamic Values

Request:

```json
{
  "message_id": "example-3",
  "message_type": "get_dynamic_values",
  "data": null
}
```

Example response:

```json
{
  "message_id": "example-3",
  "message_type": "get_dynamic_values",
  "data": {
    "width": 1920,
    "height": 1080,
    "opacity": 100,
    "gpi": false,
    "gpo": false,
    "gpio": false,
    "text_select": "VideoIPath"
  }
}
```

### Get Selected Dynamic Values

Request:

```json
{
  "message_id": "example-4",
  "message_type": "get_dynamic_values",
  "data": {
    "ids": ["width", "height"]
  }
}
```

Example response:

```json
{
  "message_id": "example-4",
  "message_type": "get_dynamic_values",
  "data": {
    "width": 1920,
    "height": 1080
  }
}
```

Unknown IDs are omitted from the response.

### Set Dynamic Values

Request:

```json
{
  "message_id": "example-5",
  "message_type": "set_dynamic_values",
  "data": {
    "values": {
      "width": 1280,
      "opacity": 75
    }
  }
}
```

A successful response contains an empty object:

```json
{
  "message_id": "example-5",
  "message_type": "set_dynamic_values",
  "data": {}
}
```

Validation failures are returned by dynamic-value ID:

```json
{
  "message_id": "example-5",
  "message_type": "set_dynamic_values",
  "data": {
    "width": "Value must be between 0 and 1920"
  }
}
```

Valid values in the same request are still applied even if another value fails validation.

### Alarms

The current implementation always returns an empty alarm list:

```json
{
  "message_id": "example-6",
  "message_type": "alarms",
  "data": []
}
```

### Error Messages

Messages with `message_type: "error"` are logged and do not receive a response.

Unsupported request types receive an error response:

```json
{
  "message_id": "example-7",
  "message_type": "error",
  "data": "Unsupported request: example_type"
}
```

## Logging

Logging is enabled at `INFO` level. The external records:

- Connection attempts
- Successful connections
- Disconnections
- Reconnection delays
- Received JSON messages
- Transmitted JSON messages
- Dynamic value changes
- Invalid JSON
- Error messages received from VideoIPath

Example:

```text
2026-09-30 10:00:00,000 Connecting to wss://vip.example.com/x-driver/v1?node_id=550e8400-e29b-41d4-a716-446655440000
2026-09-30 10:00:01,000 Connected
2026-09-30 10:00:05,000 RX {"message_id":"1","message_type":"get_dynamic_values","data":null}
2026-09-30 10:00:05,001 TX {"message_id":"1","message_type":"get_dynamic_values","data":{"width":1920}}
2026-09-30 10:01:15,000 WIDTH CHANGED: 1920 -> 1280
```

> The complete bearer token is not logged, but all transmitted and received message payloads are logged. Review the payload content before using verbose protocol logging in a production environment.

## Reconnection Behavior

When the connection ends or an exception occurs, the external:

1. Logs the exception.
2. Waits five seconds.
3. Attempts to reconnect.
4. Repeats until the process is stopped.

Dynamic values are recreated from their defaults after each successful reconnection because a new `Driver` instance is created.

## Extending the external

### Add a Numeric Parameter

Add another entry to `PARAMS`:

```python
"brightness": {
    "label": "Brightness",
    "description": "Output brightness",
    "min": 0,
    "max": 100,
    "step": 1,
    "unit": "%",
    "default": 50,
    "access": "read_write",
    "gpi_capable": False,
    "gpo_capable": False,
}
```

### Add a Boolean Parameter

```python
"enabled": {
    "type": "boolean",
    "label": "Enabled",
    "description": "Enable output processing",
    "default": True,
    "access": "read_write",
    "gpi_capable": False,
    "gpo_capable": False,
}
```

### Add a Free-Form String

Omit `values` to allow any string:

```python
"title": {
    "type": "string",
    "label": "Title",
    "description": "Displayed title",
    "default": "Example",
    "access": "read_write",
    "gpi_capable": False,
    "gpo_capable": False,
}
```

### Add Device-Specific Behavior

The update loop in `Driver.dispatch()` can call device-specific logic after a value is validated:

```python
old_value = self.params[name]
self.params[name] = new_value

if name == "gpo":
    await update_external_output(new_value)
```

## Known Limitations

- Runtime values are stored only in memory.
- Values reset to their defaults after reconnection.
- Access modes are advertised but not enforced.
- Numeric step increments are advertised but not enforced.
- Alarm responses are always empty.
- Unknown IDs in `get_dynamic_values` are silently omitted.
- The external does not actively send dynamic-value change notifications.
- The supplied code contains an unreachable `await self.tx(response)` after the `get_dynamic_values` response and `return`. It can be removed.

## Intended Use

This external is suitable for:

- External Driver prototyping
- VideoIPath integration testing
- Protocol request and response testing
- Dynamic value validation
- GPI/GPO metadata testing
- Training and demonstrations

It is a development example and should be reviewed, secured, and extended before production use.

## License

No license was specified in the supplied source code. Add a license file and update this section before distributing the project.
