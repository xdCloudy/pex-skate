import unittest

from tools.test_pex_payload_client import decode_login_entity_id


class LoginEntityIdTest(unittest.TestCase):
    def test_decodes_signed_big_endian_entity_id(self):
        self.assertEqual(decode_login_entity_id(b"\x00\x01\x02\x03"), 0x00010203)
        self.assertEqual(decode_login_entity_id(b"\xff\xff\xff\xfe"), -2)

    def test_rejects_truncated_login_packet(self):
        with self.assertRaisesRegex(ValueError, "missing its entity ID"):
            decode_login_entity_id(b"\x00\x01\x02")


if __name__ == "__main__":
    unittest.main()
