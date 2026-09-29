# VideoIPath_External_Drivers
<img width="1940" height="520" alt="image" src="https://github.com/user-attachments/assets/e236e8ea-7787-414f-a728-822985695e19" />

$\Large\color{green}{\textsf{About This Repository}}$

This guide is provided by Nevion to encourage 3rd party developers to create and contribute to the VideoIPath External Drivers repository.

Please read the Code of Conduct and Contributing Guidelines before getting started.

> [!WARNING] 
This content is provided "as is", without warranty of any kind, express or implied, including but not limited to the warranties of merchantability, fitness for a particular purpose, and non-infringement. In no event shall Nevion or its contributors be liable for any claim, damages, or other liability, whether in an action of contract, tort, or otherwise, arising from, out of, or in connection with the content or the use of or other dealings in the content.



External Driver API -  this gives third-party developers a simpler way to build on VideoIPath's wide-ranging support for broadcast, network, and monitoring devices.



$\Large\color{green}{\textsf{ What is an External Driver?}}$


An external driver is a piece of code or a process that functions as middleware, facilitating communication between VideoIPath and a media device.

The associated code is not hosted within VideoIPath and may be developed by customers independently of the VideoIPath release cycle.

Integration with VideoIPath is achieved via a self-registration mechanism. Following registration, the driver requires approval before it becomes available for use within the VideoIPath user interface.

External drivers may be implemented in any programming language and support a variety of deployment methods.

Accelerated driver development facilitates the delivery of proofs of concept (PoCs) and demonstrations.

System integrators are empowered to develop custom drivers, allowing for enhanced customization and expanding revenue opportunities within projects.

Over time, external drivers that prove reliable and effective may be promoted to official driver status.

$\Large\color{green}{\textsf{Driver API for External Drivers}}$

The Driver API provides external drivers with the following capabilities:

- Register with VideoIPath through a unique authentication token.

- Advertise Ethernet ports, sources, and destinations, along with inter-device topology.

- Advertise GPI/logic inputs and outputs.

- Raise alarms at the device, port, and endpoint levels.

- Expose parameters for use within the Panel App.

$\Large\color{green}{\textsf{Architecture}}$
<img width="965" height="503" alt="Architecture" src="https://github.com/user-attachments/assets/3722dda5-d21a-45cf-b8b4-7a671b85d09d" />



$\Large\color{green}{\textsf{Redundancy}}$
<img width="965" height="502" alt="Redundancy" src="https://github.com/user-attachments/assets/811bc383-43f1-4282-bd3d-2c8b556bacf0" />

$\Large\color{green}{\textsf{What External Drivers Can Do}}$

- General structure and device information (used to create modules, topology, GPIO inventory, known parameters, etc.)
- IP RX/TX configuration of reported senders/receivers (all transceiver functionality as format data + 1/2 IP streams).
- Read SDP from reported senders marked as supporting SDP.
- GPIO and parameter control via unified value APIs (set value, read value, and async driver value updates).
- Alarms (fetch all alarms, and async alarm updates).


$\Large\color{green}{\textsf{Limitation of Driver API}}$

**Receive switch configuration**
- External driver is for media endpoint devices

**Device maintenance functions**
- Backup and restore of configuration
- Device firmware upgrade

**Metrics**
- Reporting metrics is not supported.

**Deployment**
- VideoIPath do not handle deployment of the external driver

**Automatic driver redundancy**
- External driver is responsible for reconnecting to new server under VideoIPath node failover.

**Automatic network redundancy**
- External driver is responsible for redundancy towards media device

**Use of VideoIPath database**
- The VideoIPath internal database is a protected environment. External driver can not write to it for any reasons



## 1. Purpose and Scope

This guide explains how to build a custom VideoIPath external driver as a standalone Python process. The driver translates between a device API and the VideoIPath external-driver protocol.

>[!NOTE]
VideoIPath  <-->  External driver  <-->  Device

### External drivers can expose

- Device structure and identification

- Modules, senders, receivers, and network interfaces

- Read/write parameters values. 

- GPIO input and outputs

- Active alarms and asynchronous alarm updates

- Sender SDP

- Sender and receiver IP transport configuration

### Keep the driver focused


|$\color{green}{\textbf{\textsf{The driver should}}}$|$\color{red}{\textbf{\textsf{The driver should not}}}$|
|------------------|----------------------| 
|Connect to the VideoIPath backend as a client|Access or write to the VideoIPath database|
|Translate protocol and device state|Implement VideoIPath connection-management logic|
|Respond to every request|Depend on another VideoIPath driver|
|Report local changes asynchronously|Silently ignore unsupported requests|

## 2. Communication model

The external process is the WebSocket client. It connects to the VideoIPath backend virtual IP and exchanges JSON messages over HTTP WebSockets, with TLS support and bearer-token authentication.
```html
wss://<virtual IP>/x-driver/v1?node_id=<node ID>
```
With an HTTP header
```html
Authorization: Bearer <token>.
```


> [!IMPORTANT]
Token is created in VideoIPath cli. A token can be used by multiple devices.

```code
token --help
  -h, --help   Show help message

Subcommand: list
Usage: token list
  -h, --help   Show help message

Subcommand: gen
Usage: token gen
  -h, --help   Show help message

Subcommand: delete
Usage: token delete [token]
  -h, --help   Show help message

 trailing arguments:
  token (required)   The token to remove
```
Example
```shell
token gen
[INFO] : Successfully generated new token: "jIyxgCzWW2BZxwDM"
```

### Connection requirements

- Use a stable UUID for node_id. Keep it unchanged across restarts for the same device.

- Reconnect automatically after an unexpected disconnect.

- Disable library-level WebSocket ping messages if the protocol uses application keepalives.

- Echo keepalive requests exactly and monitor inbound traffic for loss of connectivity.

Message envelope
```json
{
  "message_id": 123,
  "message_type": "get_structure",
  "data": null
}
```


For request/response messages, preserve the request message_id. Normally, the response also uses the same message_type. Asynchronous driver-originated messages use a null message_id.

## 3. Recommended design

|driver.py|   |
|------------------|----------------------| 
| Parameter|validation and structure metadata|
|DeviceModel|parameters, alarms, endpoint state|
|Driver|WebSocket RX/TX and dispatch|
|Protocol handlers|get_structure, alarms, values, IPT|
|Connection loop|TLS, authentication, reconnect|

Use registries rather than individual variables. 

This lets the same code advertise, read, validate, update, and log many parameters.

## 4. Parameter Class and Registry
```python
from dataclasses import dataclass

@dataclass
class Parameter:
    id: str
    label: str
    description: str
    value: int
    default: int
    min_value: int
    max_value: int
    writable: bool = True

    def get_value(self) -> int:
        return self.value

    def set_value(self, new_value: int) -> tuple[int, int]:
        if isinstance(new_value, bool) or not isinstance(new_value, int):
            raise ValueError("Value must be an integer")
        if not self.writable:
            raise ValueError("Parameter is read-only")
        if not self.min_value <= new_value <= self.max_value:
            raise ValueError(
                f"Value must be between {self.min_value} and {self.max_value}"
            )

        old_value = self.value
        self.value = new_value
        print(
            f"PARAMETER {self.id}: {old_value} -> {new_value}",
            flush=True,
        )
        return old_value, new_value

    def definition(self) -> dict:
        return {
            "label": self.label,
            "description": self.description,
            "type_info": {
                "type": "number",
                "range": {
                    "from": self.min_value,
                    "to": self.max_value,
                    "step": 1,
                },
            },
            "value_info": {
                "type": "read_write" if self.writable else "read_only",
                "gpi_capable": False,
                "gpo_capable": self.writable,
            },
        }
```


Create the registry
```python
self.parameters = {
    "level": Parameter(
        id="level", label="Level",
        description="Main output level",
        value=50, default=50,
        min_value=0, max_value=100,
    ),
    "audio_gain": Parameter(
        id="audio_gain", label="Audio Gain",
        description="Simulated audio gain",
        value=10, default=10,
        min_value=0, max_value=20,
    ),
}
```


## 5. Structure and Discovery

VideoIPath initializes the external driver by requesting get_structure. 

Build dynamic-value definitions directly from the registry so the structure and runtime handlers cannot drift apart.
```python
def parameter_definitions(self) -> dict:
    return {
        parameter.id: parameter.definition()
        for parameter in self.parameters.values()
    }

def build_structure(self) -> dict:
    return {
        "label": "Python External Driver",
        "version": "1.0.0",
        "product_name": "Example Device",
        "configuration_order": "senders_first",
        "dynamic_values": self.parameter_definitions(),
        "resource_links": [],
        "modules": [
            {
                "id": "module-1",
                "label": "Module 1",
                "dynamic_values": {},
                "senders": [],
                "receivers": [],
                "generic_ports": [],
                "network_interfaces": [],
            }
        ],
    }
```
```python
if msg_type == "get_structure":
    await self.tx({
        "message_id": msg_id,
        "message_type": "get_structure",
        "data": self.build_structure(),
    })
    return
```


## 6. Reading Dynamic Values

Respect the requested IDs when they are supplied. This makes the handler work for one value or many values without returning unrelated state.
```python
def get_parameter_values(self, requested_ids=None) -> dict:
    ids = requested_ids or list(self.parameters)
    result = {}

    for parameter_id in ids:
        parameter = self.parameters.get(parameter_id)
        if parameter is None:
            continue
        result[parameter_id] = parameter.get_value()

    return result

if msg_type == "get_dynamic_values":
    data = request.get("data") or {}
    requested_ids = data.get("ids")

    await self.tx({
        "message_id": msg_id,
        "message_type": "get_dynamic_values",
        "data": self.get_parameter_values(requested_ids),
    })
    return
```


## 7. Writing Dynamic Values
```python
def set_parameter_values(self, values: dict) -> tuple[dict, dict]:
    failures = {}
    changed = {}

    for parameter_id, new_value in values.items():
        parameter = self.parameters.get(parameter_id)

        if parameter is None:
            failures[parameter_id] = "Unknown parameter"
            continue

        try:
            old_value, current_value = parameter.set_value(new_value)
            if old_value != current_value:
                changed[parameter_id] = current_value
        except ValueError as error:
            failures[parameter_id] = str(error)

    return failures, changed

if msg_type == "set_dynamic_values":
    data = request.get("data") or {}
    values = data.get("values") or {}
    failures, changed = self.set_parameter_values(values)

    await self.tx({
        "message_id": msg_id,
        "message_type": "set_dynamic_values",
        "data": failures,
    })

    if changed:
        await self.send_async_parameter_update(
            location=data.get("location"),
            values=changed,
        )
    return
```


### Asynchronous parameter updates
```python
async def send_async_parameter_update(self, location, values):
    await self.tx({
        "message_id": None,
        "message_type": "dynamic_values",
        "data": {
            "location": location,
            "values": values,
        },
    })
```


Use the exact asynchronous message_type defined by the implemented SPEC-23 backend schema. The example above follows the simulator convention used in this guide.

## 8. Keepalive, Alarms, and Errors
```python
if msg_type == "keepalive":
    await self.tx(dict(request))
    return

if msg_type == "alarms":
    await self.tx({
        "message_id": msg_id,
        "message_type": "alarms",
        "data": [],
    })
    return

if msg_type == "error":
    logging.warning("Error received from VIP: %s", request.get("data"))
    return

await self.tx({
    "message_id": msg_id,
    "message_type": "error",
    "data": f"Unsupported request: {msg_type}",
})
```


Do not answer an incoming error with another error. Doing so can create an error-response loop.

## 9. Endpoint IP Configuration

When a connection is made, VideoIPath may send set_ipt_configurations containing sender and receiver configurations. Store the accepted configuration and return separate sender and receiver error arrays.
```python
self.sender_configs = {}
self.receiver_configs = {}

if msg_type == "set_ipt_configurations":
    data = request.get("data") or {}
    module_id = data.get("module_id")

    for config in data.get("sender_configs", []):
        key = (module_id, config.get("id"), config.get("sub_index"))
        self.sender_configs[key] = config
        logging.info("Sender configuration updated: %s", key)

    for config in data.get("receiver_configs", []):
        key = (module_id, config.get("id"), config.get("sub_index"))
        self.receiver_configs[key] = config
        logging.info("Receiver configuration updated: %s", key)

    await self.tx({
        "message_id": msg_id,
        "message_type": "set_ipt_configurations",
        "data": {
            "sender_config_errors": [],
            "receiver_config_errors": [],
        },
    })
    return
```


### Configuration verification
```python
if msg_type == "check_ipt_configuration":
    # Production code should compare the requested and actual device state.
    await self.tx({
        "message_id": msg_id,
        "message_type": "check_ipt_configuration",
        "data": True,
    })
    return
```


## 10. Connection Loop
```python
async def connect_forever(args):
    ssl_context = ssl.create_default_context() if args.url.startswith("wss://") else None
    if ssl_context and args.insecure:
        ssl_context.check_hostname = False
        ssl_context.verify_mode = ssl.CERT_NONE

    separator = "&" if "?" in args.url else "?"
    url = f"{args.url}{separator}node_id={args.node_id}"
    headers = {"Authorization": f"Bearer {args.token}"}

    while True:
        try:
            async with websockets.connect(
                url,
                additional_headers=headers,
                ssl=ssl_context,
                ping_interval=None,
            ) as websocket:
                driver = Driver(websocket)
                async for message in websocket:
                    await driver.rx(message)
        except Exception:
            logging.exception("Driver disconnected")

        await asyncio.sleep(5)
```


## 11. Development and test Sequence

| Step | Milestone | Verification |
| ---- | --------- | ------------ |
| 1 | Connection | Confirm TLS, bearer authentication, node_id, and reconnect. |
| 2 | Discovery | Handle keepalive and get_structure. |
| 3 | Monitoring | Return alarms and read dynamic values. |
| 4 | Control | Validate set_dynamic_values and log changes. |
| 5 | Routing | Implement set_ipt_configurations and configuration checks. |
| 6 | Device integration | Replace simulated state with real device API calls. |
| 7 | Resilience | Add timeouts, structured logging, health checks, and persistent local state only where necessary. |

## 12. Debugging Checklist

- Log every RX and TX message with message_id and message_type.

- Pretty-print full JSON only when detailed troubleshooting is enabled.

- Confirm every request receives exactly one response.

- Never respond to an incoming error message.

- Validate integer values strictly. In Python, bool is a subclass of int, so reject boolean explicitly.

- Keep structure IDs stable and unique within their scope.

- Use --insecure only in controlled test environments.

- Compare requested IPT configuration with actual device state before reporting success in production.

## 13. Deployment Notes

- Package the driver as a small standalone service or container.

- Run it as a non-root user.

- Rotate logs and avoid storing secrets in source code.

- Add a health check when deployed in a container.

- The external process is responsible for its own deployment, operation, and resilience.

- Treat an early external-driver implementation as experimental until validated for the intended use case.

[Nevion VideoIPath](https://nevion.com/videoipath)
