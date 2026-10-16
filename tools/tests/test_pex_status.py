from __future__ import annotations

import json
import socket
import threading
import unittest

from tools import check_pex_status


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


if __name__ == "__main__":
    unittest.main()
