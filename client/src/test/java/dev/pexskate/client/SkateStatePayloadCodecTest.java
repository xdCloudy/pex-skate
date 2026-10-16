package dev.pexskate.client;

import io.netty.buffer.Unpooled;
import net.minecraft.core.RegistryAccess;
import net.minecraft.network.RegistryFriendlyByteBuf;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.HexFormat;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;

final class SkateStatePayloadCodecTest {
    @Test
    void decodesAndReencodesTheServerGeneratedV11Fixture() throws IOException {
        byte[] serverPayload = serverFixture();
        SkateStatePayload state = decode(serverPayload);

        assertEquals(11, state.version());
        assertEquals(0x01020304, state.entityId());
        assertEquals(1.25f, state.x());
        assertEquals(2.5f, state.y());
        assertEquals(-3.75f, state.z());
        assertEquals(4.0f, state.boardPosX());
        assertEquals(0.70710677f, state.boardRotZ());
        assertEquals(123.5f, state.sequenceScore());
        assertEquals(456.25f, state.lineScore());
        assertEquals(789.75f, state.totalScore());
        assertEquals("Kickflip", state.trickName());
        assertEquals(2, state.scoreFlags());
        assertEquals(true, state.mapBacked());
        assertEquals("University.skate", state.mapFilename());
        assertEquals("ab".repeat(32), state.mapSha256());
        assertEquals("cd".repeat(32), state.propsSha256());
        assertEquals(0x11223344, state.acknowledgedControlSequence());
        assertEquals(1, state.propTransforms().size());

        SkateStatePayload.PropTransform prop = state.propTransforms().getFirst();
        assertEquals(0x0102030405060708L, prop.identity());
        assertEquals(1.5f, prop.x());
        assertEquals(-2.25f, prop.y());
        assertEquals(0.125f, prop.z());
        assertEquals(0.75f, prop.yaw());
        assertEquals(10.0f, prop.pivotX());
        assertEquals(64.0f, prop.pivotY());
        assertEquals(-5.0f, prop.pivotZ());

        var encodedBuffer = new RegistryFriendlyByteBuf(Unpooled.buffer(), RegistryAccess.EMPTY);
        try {
            SkateStatePayload.CODEC.encode(encodedBuffer, state);
            byte[] encoded = new byte[encodedBuffer.readableBytes()];
            encodedBuffer.getBytes(encodedBuffer.readerIndex(), encoded);
            assertArrayEquals(serverPayload, encoded);
        } finally {
            encodedBuffer.release();
        }
    }

    @Test
    void legacyV10PropUpdatesDefaultRotationAndPivot() throws IOException {
        byte[] fixture = serverFixture();
        byte[] legacy = Arrays.copyOf(fixture, fixture.length - 16);
        legacy[0] = 10;

        SkateStatePayload state = decode(legacy);
        SkateStatePayload.PropTransform prop = state.propTransforms().getFirst();
        assertEquals(10, state.version());
        assertEquals(0.0f, prop.yaw());
        assertEquals(0.0f, prop.pivotX());
        assertEquals(0.0f, prop.pivotY());
        assertEquals(0.0f, prop.pivotZ());
    }

    private static SkateStatePayload decode(byte[] bytes) {
        var buffer = new RegistryFriendlyByteBuf(Unpooled.wrappedBuffer(bytes), RegistryAccess.EMPTY);
        try {
            SkateStatePayload decoded = SkateStatePayload.CODEC.decode(buffer);
            assertEquals(0, buffer.readableBytes(), "decoder should consume the complete server payload");
            return decoded;
        } finally {
            buffer.release();
        }
    }

    private static byte[] serverFixture() throws IOException {
        try (InputStream input = SkateStatePayloadCodecTest.class.getResourceAsStream("/pex-state-v11.hex")) {
            if (input == null) throw new IOException("missing server state fixture");
            String hex = new String(input.readAllBytes(), StandardCharsets.US_ASCII).trim();
            return HexFormat.of().parseHex(hex);
        }
    }
}
