#!/usr/bin/env python3
"""Connect an offline Minecraft 26.3 client and verify the PEX Skate hello ack."""

from __future__ import annotations

import argparse
import socket
import struct
import sys
import uuid
import zlib


PROTOCOL_VERSION = 777
SKATE_PROTOCOL_VERSION = 8
MAX_PACKET_SIZE = 32 * 1024 * 1024


def encode_varint(value: int) -> bytes:
    value &= 0xFFFFFFFF
    output = bytearray()
    while True:
        part = value & 0x7F
        value >>= 7
        if value:
            part |= 0x80
        output.append(part)
        if not value:
            return bytes(output)


def decode_varint(data: bytes, offset: int = 0) -> tuple[int, int]:
    result = 0
    for index in range(5):
        if offset + index >= len(data):
            raise ValueError("truncated Minecraft VarInt")
        byte = data[offset + index]
        result |= (byte & 0x7F) << (index * 7)
        if not byte & 0x80:
            if result & 0x80000000:
                result -= 0x100000000
            return result, offset + index + 1
    raise ValueError("invalid Minecraft VarInt")


def encode_string(value: str) -> bytes:
    encoded = value.encode("utf-8")
    return encode_varint(len(encoded)) + encoded


def read_exact(stream: socket.socket, size: int) -> bytes:
    output = bytearray()
    while len(output) < size:
        chunk = stream.recv(size - len(output))
        if not chunk:
            raise ConnectionError("server closed the connection")
        output.extend(chunk)
    return bytes(output)


def read_stream_varint(stream: socket.socket) -> int:
    data = bytearray()
    for _ in range(5):
        byte = read_exact(stream, 1)[0]
        data.append(byte)
        if byte & 0x80 == 0:
            return decode_varint(bytes(data))[0]
    raise ValueError("invalid framed Minecraft VarInt")


class MinecraftConnection:
    def __init__(self, host: str, port: int) -> None:
        self.host = host
        self.port = port
        self.stream: socket.socket | None = None
        self.compression_threshold: int | None = None

    def __enter__(self) -> MinecraftConnection:
        self.stream = socket.create_connection((self.host, self.port), timeout=5)
        self.stream.settimeout(30)
        return self

    def __exit__(self, *_: object) -> None:
        if self.stream is not None:
            self.stream.close()

    def send(self, packet_id: int, body: bytes = b"") -> None:
        if self.stream is None:
            raise RuntimeError("client is disconnected")
        payload = encode_varint(packet_id) + body
        if self.compression_threshold is not None:
            if len(payload) >= self.compression_threshold:
                payload = encode_varint(len(payload)) + zlib.compress(payload)
            else:
                payload = b"\x00" + payload
        self.stream.sendall(encode_varint(len(payload)) + payload)

    def receive(self) -> tuple[int, bytes]:
        if self.stream is None:
            raise RuntimeError("client is disconnected")
        frame_length = read_stream_varint(self.stream)
        if frame_length <= 0 or frame_length > MAX_PACKET_SIZE:
            raise ValueError(f"invalid Minecraft packet size {frame_length}")
        frame = read_exact(self.stream, frame_length)
        if self.compression_threshold is not None:
            data_length, offset = decode_varint(frame)
            if data_length:
                if data_length > MAX_PACKET_SIZE:
                    raise ValueError(f"invalid decompressed packet size {data_length}")
                payload = zlib.decompress(frame[offset:])
                if len(payload) != data_length:
                    raise ValueError("Minecraft packet compression length mismatch")
                frame = payload
            else:
                frame = frame[offset:]
        packet_id, offset = decode_varint(frame)
        return packet_id, frame[offset:]


def perform_handshake(client: MinecraftConnection, username: str) -> None:
    client.send(
        0,
        encode_varint(PROTOCOL_VERSION)
        + encode_string(client.host)
        + struct.pack(">H", client.port)
        + encode_varint(2),
    )
    offline_uuid = uuid.uuid4().bytes
    client.send(0, encode_string(username) + offline_uuid)

    while True:
        packet_id, body = client.receive()
        if packet_id == 0x00:  # Login disconnect.
            raise ConnectionError(f"server rejected offline login: {body.hex()}")
        if packet_id == 0x03:  # Set compression.
            threshold, _ = decode_varint(body)
            client.compression_threshold = threshold
        elif packet_id == 0x04:  # Login plugin request: decline optional auth extensions.
            message_id, offset = decode_varint(body)
            client.send(0x02, encode_varint(message_id) + b"\x00")
        elif packet_id == 0x05:  # Login cookie request: no stored cookie.
            key_length, offset = decode_varint(body)
            key_end = offset + key_length
            key = body[offset:key_end]
            client.send(0x04, encode_varint(len(key)) + key + b"\x00")
        elif packet_id == 0x02:  # Login success.
            client.send(0x03)  # Login acknowledged.
            client.send(0x00, config_settings())
            print("Minecraft 26.3 client authenticated in offline test mode; negotiating configuration.")
            return
        else:
            raise ValueError(f"unexpected login packet {packet_id:#x}: {body[:32].hex()}")


def config_settings() -> bytes:
    return (
        encode_string("en_us")
        + struct.pack(">b", 8)
        + encode_varint(0)
        + b"\x01\x7f"
        + encode_varint(1)
        + b"\x00\x01"
        + encode_varint(0)
    )


def config_step(client: MinecraftConnection) -> None:
    while True:
        packet_id, body = client.receive()
        print(f"Minecraft configuration packet {packet_id:#04x} ({len(body)} bytes).")
        if packet_id == 0x02:  # Disconnect.
            raise ConnectionError(f"server rejected configuration: {body[:64].hex()}")
        if packet_id == 0x03:  # Finish configuration.
            client.send(0x03)
            print("Minecraft 26.3 client entered play state; waiting for PEX Skate acknowledgement.")
            return
        if packet_id == 0x04:  # Keep alive.
            client.send(0x04, body)
        elif packet_id == 0x05:  # Ping.
            client.send(0x05, body)
        elif packet_id == 0x00:  # Cookie request.
            key_length, offset = decode_varint(body)
            key = body[offset:offset + key_length]
            client.send(0x01, encode_string(key.decode("utf-8")) + b"\x00")
        elif packet_id == 0x0E:  # Select known packs; advertise none and receive full registries.
            client.send(0x07, b"\x00")
        elif packet_id == 0x13:  # Code of conduct; accept it for this disposable local test.
            client.send(0x09)
        elif packet_id == 0x09:  # Optional resource pack request.
            if len(body) < 16:
                raise ValueError("truncated configuration resource pack request")
            client.send(0x06, body[:16] + encode_varint(1))


def play_until_ack(client: MinecraftConnection) -> None:
    saw_login = False
    while True:
        packet_id, body = client.receive()
        if packet_id == 0x20:  # Disconnect.
            raise ConnectionError(f"server disconnected the play client: {body[:64].hex()}")
        if packet_id == 0x31 and not saw_login:  # Play login.
            saw_login = True
            client.send(0x2C)  # Client loaded into the world.
            client.send(0x16, encode_string("pexskate:hello") + bytes([SKATE_PROTOCOL_VERSION]))
            client.send(0x16, encode_string("pexskate:control") + bytes.fromhex("09e77fc840c0817f0712345678"))
        elif packet_id == 0x18:  # Clientbound custom payload.
            channel_length, offset = decode_varint(body)
            channel = body[offset:offset + channel_length].decode("utf-8")
            payload = body[offset + channel_length:]
            if channel == "pexskate:hello_ack":
                if payload != bytes([SKATE_PROTOCOL_VERSION]):
                    raise ValueError(f"server acknowledged unexpected Skate protocol {payload.hex()}")
                print(f"PEX accepted Skate protocol {payload[0]} over a real Minecraft 26.3 player connection.")
                return
        elif packet_id == 0x2C:  # Keep alive.
            client.send(0x1C, body)
        elif packet_id == 0x3D:  # Ping.
            client.send(0x2D, body)
        elif packet_id == 0x48:  # Synchronize player position.
            teleport_id, _ = decode_varint(body)
            client.send(0x00, encode_varint(teleport_id))
        elif packet_id == 0x76:  # Server returned to configuration.
            client.send(0x10)  # Acknowledge configuration transition.
            config_step(client)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=25565)
    parser.add_argument("--username", default="PexSkateSmoke")
    args = parser.parse_args()
    try:
        with MinecraftConnection(args.host, args.port) as client:
            perform_handshake(client, args.username)
            config_step(client)
            play_until_ack(client)
    except (OSError, ValueError, ConnectionError, zlib.error) as error:
        print(f"PEX player payload test failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
