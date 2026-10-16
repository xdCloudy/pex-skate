#!/usr/bin/env python3
"""Check the Minecraft status handshake served by a running PEX instance."""

from __future__ import annotations

import argparse
import json
import socket
import struct
import sys


EXPECTED_PROTOCOL = 777
EXPECTED_VERSION = "26.3"


def encode_varint(value: int) -> bytes:
    encoded = bytearray()
    while True:
        part = value & 0x7F
        value >>= 7
        if value:
            part |= 0x80
        encoded.append(part)
        if not value:
            return bytes(encoded)


def read_varint(stream: socket.socket) -> int:
    result = 0
    for index in range(5):
        byte = stream.recv(1)
        if not byte:
            raise ConnectionError("server closed the connection during a VarInt")
        value = byte[0]
        result |= (value & 0x7F) << (7 * index)
        if value & 0x80 == 0:
            return result
    raise ValueError("server sent an invalid VarInt")


def read_exact(stream: socket.socket, length: int) -> bytes:
    chunks = bytearray()
    while len(chunks) < length:
        chunk = stream.recv(length - len(chunks))
        if not chunk:
            raise ConnectionError("server closed the connection before the status packet completed")
        chunks.extend(chunk)
    return bytes(chunks)


def encode_string(value: str) -> bytes:
    encoded = value.encode("utf-8")
    return encode_varint(len(encoded)) + encoded


def check_status(host: str, port: int) -> dict[str, object]:
    with socket.create_connection((host, port), timeout=3) as stream:
        stream.settimeout(3)
        handshake = (
            encode_varint(0)
            + encode_varint(EXPECTED_PROTOCOL)
            + encode_string(host)
            + struct.pack(">H", port)
            + encode_varint(1)
        )
        stream.sendall(encode_varint(len(handshake)) + handshake)
        stream.sendall(b"\x01\x00")  # Status request: packet length 1, packet id 0.

        packet_length = read_varint(stream)
        packet = read_exact(stream, packet_length)
        packet_id, offset = decode_packet_varint(packet)
        if packet_id != 0:
            raise ValueError(f"expected status response packet 0, received {packet_id}")
        json_length, string_offset = decode_packet_varint(packet, offset)
        end = string_offset + json_length
        if end != len(packet):
            raise ValueError("status response contains an invalid JSON string length")
        response = json.loads(packet[string_offset:end].decode("utf-8"))

    version = response.get("version") if isinstance(response, dict) else None
    if not isinstance(version, dict):
        raise ValueError("status response does not contain a version object")
    if version.get("protocol") != EXPECTED_PROTOCOL:
        raise ValueError(
            f"PEX advertised protocol {version.get('protocol')!r}; expected {EXPECTED_PROTOCOL}"
        )
    if version.get("name") != EXPECTED_VERSION:
        raise ValueError(
            f"PEX advertised Minecraft {version.get('name')!r}; expected {EXPECTED_VERSION}"
        )
    return response


def decode_packet_varint(packet: bytes, offset: int = 0) -> tuple[int, int]:
    result = 0
    for index in range(5):
        if offset + index >= len(packet):
            raise ValueError("status response ended during a VarInt")
        value = packet[offset + index]
        result |= (value & 0x7F) << (7 * index)
        if value & 0x80 == 0:
            return result, offset + index + 1
    raise ValueError("status response contains an invalid VarInt")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=25565)
    parser.add_argument("--quiet", action="store_true")
    args = parser.parse_args()
    try:
        response = check_status(args.host, args.port)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        if not args.quiet:
            print(f"PEX status handshake failed: {error}", file=sys.stderr)
        return 1
    if not args.quiet:
        version = response["version"]
        print(f"PEX status handshake succeeded: Minecraft {version['name']} (protocol {version['protocol']}).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
