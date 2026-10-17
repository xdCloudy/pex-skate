#!/usr/bin/env python3
"""Connect an offline Minecraft 26.3 client and smoke-test PEX Skate."""

from __future__ import annotations

import argparse
import socket
import struct
import sys
import time
import uuid
import zlib


PROTOCOL_VERSION = 777
SKATE_PROTOCOL_VERSION = 8
MAX_PACKET_SIZE = 32 * 1024 * 1024

# PEX 1.0.0's generated Minecraft 26.3 packet table.
PLAY_CLIENTBOUND_DISCONNECT = 0x20
PLAY_CLIENTBOUND_CUSTOM_PAYLOAD = 0x18
PLAY_CLIENTBOUND_KEEP_ALIVE = 0x2D
PLAY_CLIENTBOUND_LOGIN = 0x32
PLAY_CLIENTBOUND_PING = 0x3E
PLAY_CLIENTBOUND_POSITION = 0x49
PLAY_SERVERBOUND_CUSTOM_PAYLOAD = 0x16
PLAY_SERVERBOUND_CHAT_COMMAND = 0x07
PLAY_SERVERBOUND_KEEP_ALIVE = 0x1C
PLAY_SERVERBOUND_PLAYER_LOADED = 0x2C
PLAY_SERVERBOUND_PONG = 0x2D
PLAY_SERVERBOUND_TELEPORT_CONFIRM = 0x00


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


def decode_skate_state(data: bytes) -> tuple[bool, int]:
    """Read the active flag and acknowledged sequence from a v11 state payload."""
    if len(data) < 81 or data[0] != 11:
        raise ValueError("expected a complete PEX Skate state-v11 payload")
    active = data[1] != 0
    offset = 6 + 18 * 4 + 1
    trick_length = int.from_bytes(data[offset:offset + 2], "big")
    if trick_length > 192:
        raise ValueError("PEX Skate state trick name exceeds the protocol limit")
    offset += 2 + trick_length + 564  # fixed hips, skeleton, camera and rotation fields
    if offset + 3 > len(data):
        raise ValueError("truncated PEX Skate state visual payload")
    offset += 1  # map-backed flag
    map_name_length = int.from_bytes(data[offset:offset + 2], "big")
    if map_name_length > 512:
        raise ValueError("PEX Skate map filename exceeds the protocol limit")
    offset += 2 + map_name_length + 64  # filename and two SHA-256 digests
    if offset + 4 > len(data):
        raise ValueError("truncated PEX Skate state acknowledgement")
    return active, int.from_bytes(data[offset:offset + 4], "big")


def encode_push_control(sequence: int) -> bytes:
    """Build a v9 right-foot push control with a big-endian sequence number."""
    return bytes((9, 1, 0, 0, 127, 0, 0, 0, 0)) + struct.pack(">I", sequence & 0xFFFFFFFF)


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
        elif packet_id == 0x0F:  # PEX 1.0.0 select-known-packs; request full registries.
            client.send(0x07, b"\x00")
        elif packet_id == 0x13:  # Code of conduct; accept it for this disposable local test.
            client.send(0x09)
        elif packet_id == 0x09:  # Optional resource pack request.
            if len(body) < 16:
                raise ValueError("truncated configuration resource pack request")
            client.send(0x06, body[:16] + encode_varint(1))


def play_until_ack(
    client: MinecraftConnection,
    exercise_skate: bool = False,
    exercise_timeout: float = 120.0,
) -> None:
    saw_login = False
    skate_command_sent = False
    skate_active = False
    next_control_at = 0.0
    next_sequence = 0
    outstanding_sequences: set[int] = set()
    deadline = time.monotonic() + exercise_timeout
    if exercise_skate and client.stream is not None:
        client.stream.settimeout(0.25)
    while True:
        if exercise_skate and time.monotonic() >= deadline:
            missing = "an active Skate session" if not skate_active else "a consumed control sequence"
            raise TimeoutError(f"timed out waiting for {missing}")
        try:
            packet_id, body = client.receive()
        except TimeoutError:
            continue
        print(f"Minecraft play packet {packet_id:#04x} ({len(body)} bytes).")
        if packet_id == PLAY_CLIENTBOUND_DISCONNECT:
            raise ConnectionError(f"server disconnected the play client: {body[:64].hex()}")
        if packet_id == PLAY_CLIENTBOUND_LOGIN and not saw_login:
            saw_login = True
            client.send(PLAY_SERVERBOUND_PLAYER_LOADED)
            client.send(PLAY_SERVERBOUND_CUSTOM_PAYLOAD, encode_string("pexskate:hello") + bytes([SKATE_PROTOCOL_VERSION]))
            if not exercise_skate:
                client.send(PLAY_SERVERBOUND_CUSTOM_PAYLOAD, encode_string("pexskate:control") + bytes.fromhex("09e77fc840c0817f0712345678"))
        elif packet_id == PLAY_CLIENTBOUND_CUSTOM_PAYLOAD:
            channel_length, offset = decode_varint(body)
            channel = body[offset:offset + channel_length].decode("utf-8")
            payload = body[offset + channel_length:]
            if channel == "pexskate:hello_ack":
                if payload != bytes([SKATE_PROTOCOL_VERSION]):
                    raise ValueError(f"server acknowledged unexpected Skate protocol {payload.hex()}")
                print(f"PEX accepted Skate protocol {payload[0]} over a real Minecraft 26.3 player connection.")
                if not exercise_skate:
                    return
                if not skate_command_sent:
                    client.send(PLAY_SERVERBOUND_CHAT_COMMAND, encode_string("skate"))
                    skate_command_sent = True
                    print("Requested /skate; waiting for an active engine state and consumed control.")
            elif channel == "pexskate:state" and exercise_skate:
                active, acknowledged_sequence = decode_skate_state(payload)
                if active:
                    skate_active = True
                    now = time.monotonic()
                    if now >= next_control_at:
                        next_sequence = (next_sequence + 1) & 0xFFFFFFFF
                        client.send(
                            PLAY_SERVERBOUND_CUSTOM_PAYLOAD,
                            encode_string("pexskate:control") + encode_push_control(next_sequence),
                        )
                        outstanding_sequences.add(next_sequence)
                        if len(outstanding_sequences) > 256:
                            outstanding_sequences.pop()
                        next_control_at = now + 0.05
                    if acknowledged_sequence in outstanding_sequences:
                        print(
                            "PEX started a Skate session and consumed control sequence "
                            f"{acknowledged_sequence}."
                        )
                        return
        elif packet_id == PLAY_CLIENTBOUND_KEEP_ALIVE:
            client.send(PLAY_SERVERBOUND_KEEP_ALIVE, body)
        elif packet_id == PLAY_CLIENTBOUND_PING:
            client.send(PLAY_SERVERBOUND_PONG, body)
        elif packet_id == PLAY_CLIENTBOUND_POSITION:
            teleport_id, _ = decode_varint(body)
            client.send(PLAY_SERVERBOUND_TELEPORT_CONFIRM, encode_varint(teleport_id))
        elif packet_id == 0x76:  # Server returned to configuration.
            client.send(0x10)  # Acknowledge configuration transition.
            config_step(client)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=25565)
    parser.add_argument("--username", default="PexSkateSmoke")
    parser.add_argument(
        "--exercise-skate",
        action="store_true",
        help="send /skate and require active state plus a server-consumed control (requires converted game data)",
    )
    parser.add_argument("--exercise-timeout", type=float, default=120.0)
    args = parser.parse_args()
    try:
        with MinecraftConnection(args.host, args.port) as client:
            perform_handshake(client, args.username)
            config_step(client)
            play_until_ack(client, args.exercise_skate, args.exercise_timeout)
    except (OSError, ValueError, ConnectionError, TimeoutError, zlib.error) as error:
        print(f"PEX player payload test failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
