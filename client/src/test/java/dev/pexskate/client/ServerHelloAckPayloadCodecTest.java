package dev.pexskate.client;

import io.netty.buffer.Unpooled;
import net.minecraft.core.RegistryAccess;
import net.minecraft.network.RegistryFriendlyByteBuf;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.HexFormat;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;

final class ServerHelloAckPayloadCodecTest {
    @Test
    void serverHelloAckUsesTheSharedOneByteProtocolFixture() throws IOException {
        byte[] expected = readFixture();
        ServerHelloAckPayload payload = new ServerHelloAckPayload((byte) 8);

        var encoded = new RegistryFriendlyByteBuf(Unpooled.buffer(), RegistryAccess.EMPTY);
        try {
            ServerHelloAckPayload.CODEC.encode(encoded, payload);
            byte[] actual = new byte[encoded.readableBytes()];
            encoded.getBytes(encoded.readerIndex(), actual);
            assertArrayEquals(expected, actual);
        } finally {
            encoded.release();
        }

        var received = new RegistryFriendlyByteBuf(Unpooled.wrappedBuffer(expected), RegistryAccess.EMPTY);
        try {
            assertEquals(payload, ServerHelloAckPayload.CODEC.decode(received));
            assertEquals(0, received.readableBytes());
        } finally {
            received.release();
        }
    }

    private static byte[] readFixture() throws IOException {
        try (InputStream input = ServerHelloAckPayloadCodecTest.class.getResourceAsStream("/pex-hello-ack-v1.hex")) {
            if (input == null) throw new IOException("missing hello acknowledgment fixture");
            return HexFormat.of().parseHex(new String(input.readAllBytes(), StandardCharsets.US_ASCII).trim());
        }
    }
}
