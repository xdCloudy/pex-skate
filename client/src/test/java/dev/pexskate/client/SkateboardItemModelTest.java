package dev.pexskate.client;

import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

class SkateboardItemModelTest {
    @Test
    void usesTexturesFromTheSingleItemAtlas() throws IOException {
        try (InputStream input = getClass().getResourceAsStream(
                "/assets/pexskate/models/item/skateboard.json")) {
            assertTrue(input != null, "skateboard item model resource is present");
            String model = new String(input.readAllBytes(), StandardCharsets.UTF_8);
            Matcher texture = Pattern.compile(
                    "\\\"(?:deck|truck|wheel|particle)\\\"\\s*:\\s*\\\"([^\\\"]+)\\\"")
                    .matcher(model);
            int count = 0;
            while (texture.find()) {
                assertTrue(texture.group(1).contains(":item/"),
                        "cuboid item texture must be in the item atlas: " + texture.group(1));
                count++;
            }
            assertEquals(4, count, "deck, truck, wheel, and particle textures are declared");
        }
    }

    @Test
    void keepsEveryCuboidInsideTheSixteenPixelModelBounds() throws IOException {
        try (InputStream input = getClass().getResourceAsStream(
                "/assets/pexskate/models/item/skateboard.json")) {
            assertTrue(input != null, "skateboard item model resource is present");
            String model = new String(input.readAllBytes(), StandardCharsets.UTF_8);
            Matcher coordinates = Pattern.compile(
                    "\\\"(?:from|to)\\\"\\s*:\\s*\\[([^\\]]+)\\]")
                    .matcher(model);
            int cuboidCorners = 0;
            while (coordinates.find()) {
                String[] axes = coordinates.group(1).split(",");
                assertEquals(3, axes.length, "each cuboid corner has three coordinates");
                for (String axis : axes) {
                    double coordinate = Double.parseDouble(axis.trim());
                    assertTrue(coordinate >= 0.0 && coordinate <= 16.0,
                            "default face UVs must stay inside the 16x16 atlas: " + coordinate);
                }
                cuboidCorners++;
            }
            assertTrue(cuboidCorners > 0, "model contains cuboid corners");
        }
    }

    @Test
    void keepsEveryFaceUvInsideTheSixteenPixelTextureBounds() throws IOException {
        try (InputStream input = getClass().getResourceAsStream(
                "/assets/pexskate/models/item/skateboard.json")) {
            assertTrue(input != null, "skateboard item model resource is present");
            String model = new String(input.readAllBytes(), StandardCharsets.UTF_8);
            Matcher uvs = Pattern.compile("\\\"uv\\\"\\s*:\\s*\\[([^\\]]+)\\]")
                    .matcher(model);
            int faceCount = 0;
            while (uvs.find()) {
                String[] coordinates = uvs.group(1).split(",");
                assertEquals(4, coordinates.length, "each face UV has four coordinates");
                for (String coordinate : coordinates) {
                    double value = Double.parseDouble(coordinate.trim());
                    assertTrue(value >= 0.0 && value <= 16.0,
                            "face UV must stay inside the 16x16 texture: " + value);
                }
                faceCount++;
            }
            assertEquals(42, faceCount, "every face of all seven cuboids has explicit UVs");
        }
    }
}
