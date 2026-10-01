import asyncio
import argparse
import json
import logging
import ssl
import uuid

import websockets


# ------------------------------------------------------
# Device model
# ------------------------------------------------------
# Edit this table to add/remove/rename parameters.
# ------------------------------------------------------
PARAMS = {
    "width": {
        "label": "Width",
        "description": "Output width",
        "min": 0, "max": 1920, "step": 1, "unit": "px",
        "default": 1920,
        "access": "read_write",
        "gpi_capable": False,
        "gpo_capable": False,
    },
    "height": {
        "label": "Height",
        "description": "Output height",
        "min": 0, "max": 1080, "step": 1, "unit": "px",
        "default": 1080,
        "access": "read_write",
        "gpi_capable": False,
        "gpo_capable": False,
    },
    "opacity": {
        "label": "Opacity",
        "description": "Overlay opacity",
        "min": 0, "max": 100, "step": 1, "unit": "%",
        "default": 100,
        "access": "read_write",
        "gpi_capable": False,
        "gpo_capable": False,
    },
    "gpi": {
        "type": "boolean",
        "label": "Logic in",
        "description": "Show or hide the logo overlay",
        "default": False,
        "access": "read_only",
        "gpi_capable": True,
        "gpo_capable": False,
    },
    "gpo": {
        "type": "boolean",
        "label": "Logic out",
        "description": "Show or hide the logo overlay",
        "default": False,
        "access": "write_only",
        "gpi_capable": False,
        "gpo_capable": True,
    },
    "gpio": {
        "type": "boolean",
        "label": "Logic in/out",
        "description": "Show or hide the logo overlay",
        "default": False,
        "access": "read_write",
        "gpi_capable": True,
        "gpo_capable": True,
    },
    "text_select": {
        "type": "string",
        "label": "Product",
        "description": "Which product to display",
        "values": ["VideoIPath", "Virtuoso"],   # omit for free-form text
        "default": "VideoIPath",
        "access": "read_write",
        "gpi_capable": False,
        "gpo_capable": False,
    }
}

# ------------------------------------------------------

def build_type_info(p):
    ptype = p.get("type", "number")

    if ptype == "boolean":
        return {"type": "boolean"}

    if ptype == "string":
        info = {"type": "string"}
        if "values" in p:
            info["values"] = list(p["values"])
        return info

    return {
        "type": "number",
        "range": {
            "from": p["min"],
            "to": p["max"],
            "step": p["step"],
            "unit": p["unit"],
        },
    }

def build_value_info(p):
    return {
        "type": p.get("access", "read_write"),
        "gpi_capable": p.get("gpi_capable", False),
        "gpo_capable": p.get("gpo_capable", False),
    }

def build_param_dynamic_values():
    return {
        name: {
            "label": p["label"],
            "description": p["description"],
            "type_info": build_type_info(p),
            "value_info": build_value_info(p),
        }
        for name, p in PARAMS.items()
    }

def build_device_structure():
    return {
        "label": "External driver",
        "version": "1.0.0",
        "product_name": "External driver device",
        "configuration_order": "senders_first",
        "resource_links": [],
        "dynamic_values": build_param_dynamic_values(),
        "modules": [
            {
                "id": "module-1",
                "label": "Module 1",
                "dynamic_values": {},
                "senders": [
                    {
                        "id": "sender-video",
                        "label": "Sender video",
                        "dynamic_values": {},
                        "topology_info": {
                            "format_type": "Video",
                            "sdp_support": True,
                            "sips": False,
                            "interface_options": [
                                {
                                    "module_id": "module-1",
                                    "port_id": "eth-1"
                                }
                            ]
                        }
                    },
                    {
                        "id": "sender-audio",
                        "label": "Sender audio",
                        "dynamic_values": {},
                        "topology_info": {
                            "format_type": "Audio",
                            "sdp_support": True,
                            "sips": False,
                            "interface_options": [
                                {
                                    "module_id": "module-1",
                                    "port_id": "eth-1"
                                }
                            ]
                        }
                    }
                ],
                "receivers": [
                    {
                        "id": "receiver-video",
                        "label": "Receiver video",
                        "dynamic_values": {},
                        "topology_info": {
                            "format_type": "Video",
                            "sdp_support": True,
                            "sips": False,
                            "interface_options": [
                                {
                                    "module_id": "module-1",
                                    "port_id": "eth-1"
                                }
                            ]
                        }
                    },
                    {
                        "id": "receiver-audio",
                        "label": "Receiver audio",
                        "dynamic_values": {},
                        "topology_info": {
                            "format_type": "Video",
                            "sdp_support": True,
                            "sips": False,
                            "interface_options": [
                                {
                                    "module_id": "module-1",
                                    "port_id": "eth-1"
                                }
                            ]
                        }
                    }
                ],
                "generic_ports": [],
                "network_interfaces": [
                    {
                        "type": "single",
                        "id": "eth-1",
                        "label": "IP Interface",
                        "known_address": None,
                        "dynamic_values": {}
                    }
                ]
            }
        ]
    }

def normalize_value(spec, value):
    """Return (normalized_value, error). error is None if valid."""
    ptype = spec.get("type", "number")

    if ptype == "boolean":
        # VIP may send 0/1 as well as true/false
        if isinstance(value, bool):
            return value, None
        if isinstance(value, int) and value in (0, 1):
            return bool(value), None
        return None, "Value must be a boolean"

    if ptype == "string":
        if not isinstance(value, str):
            return None, "Value must be a string"
        if "values" in spec and value not in spec["values"]:
            return None, f"Value must be one of {spec['values']}"
        return value, None

    if isinstance(value, bool) or not isinstance(value, int):
        return None, "Value must be an integer"
    if not (spec["min"] <= value <= spec["max"]):
        return None, f"Value must be between {spec['min']} and {spec['max']}"
    return value, None

def format_msg(direction, msg):
    arrow = "TX: " if direction == "TX" else "RX: "
    header = f"{arrow} {msg.get('message_type')}  (id={msg.get('message_id')})"
    data = msg.get("data")
    if data is None or data == [] or data == {}:
        return header
    body = json.dumps(data, indent=2)
    body = "\n".join("    " + line for line in body.splitlines())
    return f"{header}\n{body}"

# ------------------------------------------------------
# Driver
# ------------------------------------------------------
class Driver:

    def __init__(self, websocket):
        self.websocket = websocket
        self.device_structure = build_device_structure()
        self.params = {name: p["default"] for name, p in PARAMS.items()}

    async def tx(self, msg):

        payload = json.dumps(msg)

        if msg.get("message_type") == "get_structure":
            logging.info("%s", format_msg("TX", msg))
        elif msg.get("message_type") != "keepalive":
                logging.info("TX: %s", payload)
        
        await self.websocket.send(payload)

    async def send_async_values_changed(self, values):
        """Driver-initiated set_dynamic_values towards VIP."""
        if not values:
            return

        await self.tx({
            "data": {
                "location": None,
                "values": values,
            },
            "message_id": None,
            "message_type": "async_values_changed"
        })
    
    async def rx(self, raw):
        
        try:
            msg = json.loads(raw)
        except Exception as ex:
            logging.error("Bad JSON: %s", ex)
            return
        if msg.get("message_type") != "keepalive":
            logging.info("RX: %s", raw)
        

        await self.dispatch(msg)

    async def dispatch(self, request):

        msg_id = request.get("message_id")
        msg_type = request.get("message_type")

        #
        # KEEPALIVE
        #
        if msg_type == "keepalive":

            response = {
                "message_id": msg_id,
                "message_type": "keepalive",
                "data": None
            }

            await self.tx(response)
            return

        #
        # HANDSHAKE
        #
        if msg_type == "get_structure":

            response = {
                "message_id": msg_id,
                "message_type": "get_structure",
                "data": self.device_structure
            }

            await self.tx(response)
            return

        #
        # ALARMS
        #
        if msg_type == "alarms":

            response = {
                "message_id": msg_id,
                "message_type": "alarms",
                "data": []
            }

            await self.tx(response)
            return
        #
        # ERROR
        #
        if msg_type == "error":
            logging.warning(
            "Received error from VIP: %s",
            request.get("data")
            )
            return

        #
        # Get PARAMETER and LOGIC
        #
        if msg_type == "get_dynamic_values":

            ids = (request.get("data") or {}).get("ids")
            if ids:
                data = {i: self.params[i] for i in ids if i in self.params}
            else:
                data = dict(self.params)

            await self.tx({
                "message_id": msg_id,
                "message_type": "get_dynamic_values",
                "data": data,
            })
            return

            await self.tx(response)
            return
        #
        # Set PARAMETER and LOGIC
        #
        if msg_type == "set_dynamic_values":

            failures = {}
            applied = {}
            values = (request.get("data") or {}).get("values", {})

            for name, raw_value in values.items():

                spec = PARAMS.get(name)
                if spec is None:
                    failures[name] = "Unknown dynamic value"
                    continue

                new_value, error = normalize_value(spec, raw_value)
                if error:
                    failures[name] = error
                    continue

                old_value = self.params[name]
                self.params[name] = new_value
                applied[name] = new_value
                logging.info("%s CHANGED: %s -> %s", name.upper(), old_value, new_value)

            # 1) reply to the request
            await self.tx({
                "message_id": msg_id,
                "message_type": "set_dynamic_values",
                "data": failures,
            })

            # 2) send our own set_dynamic_values back for what was applied to update VideoIPath
            await self.send_async_values_changed(applied)
            return

        #
        # EVERYTHING ELSE
        #
        response = {
            "message_id": msg_id,
            "message_type": "error",
            "data": f"Unsupported request: {msg_type}"
        }

        await self.tx(response)

# ------------------------------------------------------
# Connection loop
# ------------------------------------------------------
async def connect_forever(args):

    ssl_context = None

    if args.url.startswith("wss://"):

        ssl_context = ssl.create_default_context()

        if args.insecure:
            ssl_context.check_hostname = False
            ssl_context.verify_mode = ssl.CERT_NONE

    url = f"{args.url}?node_id={args.node_id}"

    headers = {
        "Authorization": f"Bearer {args.token}"
    }

    while True:

        try:

            logging.info("Connecting to %s", url)

            async with websockets.connect(
                url,
                additional_headers=headers,
                ssl=ssl_context,
                ping_interval=None
            ) as websocket:

                logging.info("Connected")

                driver = Driver(websocket)

                async for message in websocket:
                    await driver.rx(message)

        except Exception as ex:

            logging.error("Disconnected: %s", ex)

        logging.info("Reconnect in 5 seconds")
        await asyncio.sleep(5)

# ------------------------------------------------------
# CLI
# ------------------------------------------------------
def parse_args():

    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--url",
        required=True,
        help="wss://vip/x-driver/v1"
    )

    parser.add_argument(
        "--token",
        required=True
    )

    parser.add_argument(
        "--node-id",
        required=True
    )

    parser.add_argument(
        "--insecure",
        action="store_true"
    )

    return parser.parse_args()

# ------------------------------------------------------
# Main
# ------------------------------------------------------

async def main():

    args = parse_args()

    try:
        uuid.UUID(args.node_id)
    except Exception:
        raise SystemExit("node-id must be a valid UUID")

    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(message)s"
    )

    await connect_forever(args)

if __name__ == "__main__":
    asyncio.run(main())
