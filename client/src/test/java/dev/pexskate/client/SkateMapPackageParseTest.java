package dev.pexskate.client;

import net.minecraft.world.phys.Vec3;
import org.junit.jupiter.api.Test;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.regex.Pattern;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

class SkateMapPackageParseTest {
    @Test
    void parsesConvertedIsoMapWhenConfigured() throws IOException {
        String configuredMaps = System.getProperty("pex.skate.integration.map",
                System.getenv("PEX_SKATE_INTEGRATION_MAP"));
        assumeTrue(configuredMaps != null && !configuredMaps.isBlank(), "No converted ISO map configured");

        for (String mapPath : configuredMaps.split(Pattern.quote(File.pathSeparator))) {
            Path path = Path.of(mapPath);
            SkateMapModel.ParsedModel parsed = SkateMapModel.parse(
                    Files.readAllBytes(path), path.getFileName().toString());

            assertFalse(parsed.mapName().isBlank(), path.toString());
            assertFalse(parsed.textures().isEmpty(), path.toString());
            assertFalse(parsed.primitives().isEmpty(), path.toString());
            assertTrue(parsed.primitives().stream().anyMatch(primitive -> primitive.indices().length >= 3),
                    path.toString());

            Path propsPath = path.getParent().getParent().resolve("assets/private/native-props")
                    .resolve(path.getFileName());
            if (Files.isRegularFile(propsPath)) {
                SkateMapModel.ParsedModel props = SkateMapModel.parse(
                        Files.readAllBytes(propsPath), path.getFileName() + "-props");
                assertFalse(props.primitives().isEmpty(), propsPath.toString());
                assertTrue(props.primitives().stream().anyMatch(primitive -> primitive.propInstanceId() != 0),
                        propsPath.toString());
                SkateMapModel.ParsedModel merged = SkateMapModel.mergeProps(parsed, props);
                assertEquals(parsed.primitives().size() + props.primitives().size(), merged.primitives().size());
            }
            System.gc();
        }
    }

    @Test
    void parsesAuthoredMaterialAndTangentFrameFromSkate14Package() throws IOException {
        SkateMapModel.ParsedModel parsed = SkateMapModel.parse(syntheticMap(), "synthetic.skate");

        assertEquals("Synthetic District", parsed.mapName());
        assertEquals(2, parsed.textures().size());
        assertEquals(1, parsed.primitives().size());
        SkateMapModel.PendingPrimitive primitive = parsed.primitives().get(0);
        assertEquals(1.0f, primitive.emissive());
        assertEquals(0.0f, primitive.color()[0], 0.001f);
        assertEquals(0.0f, primitive.normals()[0], 0.01f);
        assertEquals(0.0f, primitive.normals()[1], 0.01f);
        assertEquals(1.0f, primitive.normals()[2], 0.01f);
        assertEquals(0.5, SkateMapModel.segmentTriangleFraction(
                new Vec3(0.25, 0.25, -1.0), new Vec3(0.25, 0.25, 1.0),
                primitive.positions()[0], primitive.positions()[1], primitive.positions()[2],
                primitive.positions()[3], primitive.positions()[4], primitive.positions()[5],
                primitive.positions()[6], primitive.positions()[7], primitive.positions()[8],
                1.0), 0.0001);
    }

    private static byte[] syntheticMap() throws IOException {
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        output.write("SKATE14\0".getBytes(StandardCharsets.US_ASCII));
        u32(output, 0x12345678);
        string(output, "Synthetic District");
        for (int i = 0; i < 3 + 1 + 45; i++) f32(output, 0.0f);
        for (int count : new int[] {1, 2, 3, 3, 0, 0, 0, 0, 0}) u32(output, count);

        string(output, "synthetic_material");
        u32(output, 1);
        f32(output, 0.82f); f32(output, 0.0f);
        f32(output, 0.0f); f32(output, 0.0f); f32(output, 0.0f);
        f32(output, 0.68f); f32(output, 1.0f);
        u32(output, 1); // albedo texture
        u32(output, 0); // baked lightmap
        f32(output, 0.0f); // lightmap strength
        u32(output, 2); // normal map
        u32(output, 0); u32(output, 0); // ORM and emissive textures
        u32(output, 0); // opaque alpha mode
        f32(output, 0.5f); // alpha cutoff
        u32(output, 0); u32(output, 0); u32(output, 0); // material behavior IDs
        u32(output, 0); // depth layer
        u32(output, 0); // no retail definition

        texture(output, "albedo", new byte[] {(byte) 255, (byte) 255, (byte) 255, (byte) 255});
        texture(output, "normal", new byte[] {(byte) 255, (byte) 128, (byte) 128, (byte) 255});

        ByteArrayOutputStream vertices = new ByteArrayOutputStream();
        vertex(vertices, 0, 0, 0, 0, 1, 0, 0, 0);
        vertex(vertices, 1, 0, 0, 0, 1, 0, 1, 0);
        vertex(vertices, 0, 1, 0, 0, 1, 0, 0, 1);
        stored(output, vertices.toByteArray());
        ByteArrayOutputStream indices = new ByteArrayOutputStream();
        u32(indices, 0); u32(indices, 1); u32(indices, 2);
        stored(output, indices.toByteArray());
        stored(output, new byte[0]);
        u32(output, 0); // map extensions
        return output.toByteArray();
    }

    private static void vertex(ByteArrayOutputStream output,
                               float x, float y, float z, float nx, float ny, float nz, float u, float v)
            throws IOException {
        f32(output, x); f32(output, y); f32(output, z);
        f32(output, nx); f32(output, ny); f32(output, nz);
        f32(output, u); f32(output, v);
        f32(output, 0); f32(output, 0); // lightmap UV
        u32(output, 1); // one-based material index
        f32(output, 0); f32(output, 0); // decal UV
        output.write(new byte[] {127, 0, 0, 127}); // binormal and tangent handedness
    }

    private static void texture(ByteArrayOutputStream output, String name, byte[] rgba) throws IOException {
        string(output, name);
        u32(output, 1); u32(output, 1); u32(output, 1);
        stored(output, rgba);
    }

    private static void stored(ByteArrayOutputStream output, byte[] data) throws IOException {
        u32(output, 0);
        u32(output, data.length);
        output.write(data);
    }

    private static void string(ByteArrayOutputStream output, String value) throws IOException {
        byte[] encoded = value.getBytes(StandardCharsets.UTF_8);
        u32(output, encoded.length);
        output.write(encoded);
    }

    private static void f32(ByteArrayOutputStream output, float value) throws IOException {
        u32(output, Float.floatToRawIntBits(value));
    }

    private static void u32(ByteArrayOutputStream output, int value) throws IOException {
        output.write(value & 0xff);
        output.write((value >>> 8) & 0xff);
        output.write((value >>> 16) & 0xff);
        output.write((value >>> 24) & 0xff);
    }
}
