from __future__ import annotations

import io
import json
import socket
import struct
import threading
import unittest
from contextlib import redirect_stdout

from tools import check_pex_status
from tools import test_pex_payload_client


def read_frame(stream: socket.socket) -> bytes:
    length = check_pex_status.read_varint(stream)
    return check_pex_status.read_exact(stream, length)


class PexStatusHandshakeTests(unittest.TestCase):
    def test_varint_round_trips(self) -> None:
        for value in (0, 1, 127, 128, 777, 2**31 - 1):
            with self.subTest(value=value):
                packet = check_pex_status.encode_varint(value)
                decoded, offset = check_pex_status.decode_packet_varint(packet)
                self.assertEqual(decoded, value)
                self.assertEqual(offset, len(packet))

    def test_status_handshake_checks_server_protocol(self) -> None:
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        host, port = listener.getsockname()
        response = {
            "version": {"name": "26.3", "protocol": 777},
            "players": {"max": 20, "online": 0},
            "description": {"text": "test"},
        }
        failure: list[BaseException] = []

        def serve_status() -> None:
            try:
                with listener, listener.accept()[0] as connection:
                    connection.settimeout(2)
                    read_frame(connection)  # Handshake.
                    request = read_frame(connection)
                    self.assertEqual(request, b"\x00")
                    json_bytes = json.dumps(response).encode("utf-8")
                    body = b"\x00" + check_pex_status.encode_varint(len(json_bytes)) + json_bytes
                    connection.sendall(check_pex_status.encode_varint(len(body)) + body)
            except BaseException as error:
                failure.append(error)

        server = threading.Thread(target=serve_status)
        server.start()
        try:
            actual = check_pex_status.check_status(host, port)
        finally:
            server.join(timeout=3)
        self.assertFalse(server.is_alive(), "mock status server did not finish")
        if failure:
            raise failure[0]
        self.assertEqual(actual, response)


class MinecraftClientProtocolTests(unittest.TestCase):
    def test_varints_round_trip_for_protocol_packet_ids_and_signed_values(self) -> None:
        values = (0, 1, 127, 128, 777, 2**31 - 1, -1)
        for value in values:
            with self.subTest(value=value):
                packet = test_pex_payload_client.encode_varint(value)
                decoded, offset = test_pex_payload_client.decode_varint(packet)
                self.assertEqual(decoded, value)
                self.assertEqual(offset, len(packet))

    def test_state_v11_decoder_reads_active_session_and_consumed_control(self) -> None:
        filename = b"University.skate"
        payload = bytearray((11, 1))
        payload.extend(struct.pack(">I", 42))
        payload.extend(b"\0" * (18 * 4 + 1))
        payload.extend(struct.pack(">H", 0))
        payload.extend(b"\0" * 564)
        payload.extend(b"\1")
        payload.extend(struct.pack(">H", len(filename)))
        payload.extend(filename)
        payload.extend(b"\0" * 64)
        payload.extend(struct.pack(">I", 0x12345678))

        decoded = test_pex_payload_client.decode_skate_state(bytes(payload))
        self.assertTrue(decoded.active)
        self.assertEqual(decoded.entity_id, 42)
        self.assertEqual(decoded.root, (0.0, 0.0, 0.0))
        self.assertEqual(decoded.velocity, (0.0, 0.0, 0.0))
        self.assertEqual(decoded.acknowledged_sequence, 0x12345678)

    def test_state_v11_decoder_rejects_truncated_or_unexpected_payloads(self) -> None:
        with self.assertRaisesRegex(ValueError, "state-v11"):
            test_pex_payload_client.decode_skate_state(bytes((10,)) + b"\0" * 100)
        with self.assertRaisesRegex(ValueError, "truncated"):
            test_pex_payload_client.decode_skate_state(bytes((11, 1)) + b"\0" * 79)

    def test_push_control_uses_version_nine_layout_and_big_endian_sequence(self) -> None:
        payload = test_pex_payload_client.encode_push_control(0x12345678)
        self.assertEqual(payload, bytes((9, 1, 0, 0, 127, 0, 0, 0, 0)) + bytes.fromhex("12345678"))
        flick = test_pex_payload_client.encode_control(1, right_x=-1.0, right_y=1.0)
        self.assertEqual(flick, bytes((9, 0, 0, 0, 0, 0, 129, 127, 0)) + bytes.fromhex("00000001"))

    def test_gameplay_smoke_requests_skate_and_waits_for_control_ack(self) -> None:
        def state_packet(acknowledged_sequence: int, root_x: float = 0.0,
                         velocity_x: float = 0.0, trick_name: str = "",
                         entity_id: int = 42) -> bytes:
            state = bytearray((11, 1))
            state.extend(struct.pack(">I", entity_id))
            state.extend(struct.pack(">18f", root_x, 0.0, 0.0,
                                     0.0, 0.0, 0.0,
                                     0.0, 0.0, 0.0, 1.0,
                                     velocity_x, 0.0, 0.0,
                                     1.0 if trick_name else 0.0, 0.0,
                                     1.0 if trick_name else 0.0, 1.0, 0.0))
            state.extend(b"\0")
            encoded_trick = trick_name.encode("utf-8")
            state.extend(struct.pack(">H", len(encoded_trick)))
            state.extend(encoded_trick)
            state.extend(b"\0" * 564)
            state.extend(b"\0\0\0")  # no map selection
            state.extend(b"\0" * 64)  # map and prop digests
            state.extend(struct.pack(">I", acknowledged_sequence))
            return test_pex_payload_client.encode_string("pexskate:state") + state

        packets = [
            (test_pex_payload_client.PLAY_CLIENTBOUND_LOGIN, struct.pack(">i", 42)),
            (
                test_pex_payload_client.PLAY_CLIENTBOUND_CUSTOM_PAYLOAD,
                test_pex_payload_client.encode_string("pexskate:hello_ack") + bytes((8,)),
            ),
            (test_pex_payload_client.PLAY_CLIENTBOUND_CUSTOM_PAYLOAD, state_packet(0)),
            # This remote snapshot reuses the local control sequence and has
            # an obviously different position; it must not complete the test.
            (test_pex_payload_client.PLAY_CLIENTBOUND_CUSTOM_PAYLOAD,
             state_packet(1, 100.0, 100.0, "Remote", entity_id=17)),
            (test_pex_payload_client.PLAY_CLIENTBOUND_CUSTOM_PAYLOAD,
             state_packet(1, 1.0, 1.0, "Ollie")),
        ]

        class FakeConnection:
            stream = None

            def __init__(self) -> None:
                self.sent: list[tuple[int, bytes]] = []
                self.input_last_sequence = 0

            def receive(self) -> tuple[int, bytes]:
                return packets.pop(0)

            def send(self, packet_id: int, body: bytes = b"") -> None:
                self.sent.append((packet_id, body))

            def start_gameplay_input_stream(
                self, exercise_duration: float, ollie_points: list[tuple[float, float]] | None,
                control_only: bool,
            ) -> None:
                control = test_pex_payload_client.encode_push_control(1)
                self.send(
                    test_pex_payload_client.PLAY_SERVERBOUND_CUSTOM_PAYLOAD,
                    test_pex_payload_client.encode_string("pexskate:control") + control,
                )
                self.input_last_sequence = 1

        client = FakeConnection()
        output = io.StringIO()
        with redirect_stdout(output):
            test_pex_payload_client.play_until_ack(
                client, exercise_skate=True, exercise_timeout=1, exercise_duration=0
            )
        self.assertIn("trick=Ollie", output.getvalue())
        self.assertIn("states=2", output.getvalue())

        self.assertIn(
            (test_pex_payload_client.PLAY_SERVERBOUND_CHAT_COMMAND,
             test_pex_payload_client.encode_string("skate")),
            client.sent,
        )
        control_packets = [
            body for packet_id, body in client.sent
            if packet_id == test_pex_payload_client.PLAY_SERVERBOUND_CUSTOM_PAYLOAD
            and body.startswith(test_pex_payload_client.encode_string("pexskate:control"))
        ]
        self.assertEqual(len(control_packets), 1)
        control_channel = test_pex_payload_client.encode_string("pexskate:control")
        self.assertEqual(control_packets[0][len(control_channel):], test_pex_payload_client.encode_push_control(1))


if __name__ == "__main__":
    unittest.main()
