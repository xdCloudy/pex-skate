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

final class ControlPayloadCodecTest {
    @Test
    void clientControlV9MatchesTheRustServerDecoderFixture() throws IOException {
        byte[] expected = readFixture();
        ControlPayload control = new ControlPayload(
                (byte) 9, (byte) 0xe7, (byte) 0x7f, (byte) 0xc8,
                (byte) 0x40, (byte) 0xc0, (byte) 0x81, (byte) 0x7f,
                (byte) 0x07, 0x12345678);

        var encoded = new RegistryFriendlyByteBuf(Unpooled.buffer(), RegistryAccess.EMPTY);
        try {
            ControlPayload.CODEC.encode(encoded, control);
            byte[] actual = new byte[encoded.readableBytes()];
            encoded.getBytes(encoded.readerIndex(), actual);
            assertArrayEquals(expected, actual);
        } finally {
            encoded.release();
        }

        var received = new RegistryFriendlyByteBuf(Unpooled.wrappedBuffer(expected), RegistryAccess.EMPTY);
        try {
            ControlPayload decoded = ControlPayload.CODEC.decode(received);
            assertEquals(control, decoded);
            assertEquals(0, received.readableBytes());
        } finally {
            received.release();
        }
    }

    private static byte[] readFixture() throws IOException {
        try (InputStream input = ControlPayloadCodecTest.class.getResourceAsStream("/pex-control-v9.hex")) {
            if (input == null) throw new IOException("missing client control fixture");
            return HexFormat.of().parseHex(new String(input.readAllBytes(), StandardCharsets.US_ASCII).trim());
        }
    }
}
