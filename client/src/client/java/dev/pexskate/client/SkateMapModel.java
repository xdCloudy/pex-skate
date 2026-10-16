package dev.pexskate.client;

import com.mojang.blaze3d.platform.NativeImage;
import com.mojang.blaze3d.vertex.PoseStack;
import com.mojang.blaze3d.vertex.VertexConsumer;
import net.fabricmc.fabric.api.client.rendering.v1.level.LevelRenderContext;
import net.fabricmc.fabric.api.client.rendering.v1.level.LevelRenderEvents;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.rendertype.RenderTypes;
import net.minecraft.client.renderer.texture.DynamicTexture;
import net.minecraft.resources.Identifier;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.LightLayer;
import net.minecraft.world.phys.Vec3;
import org.joml.Vector3f;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import java.util.concurrent.ConcurrentHashMap;
import java.util.zip.InflaterInputStream;

/** Static retail district renderer for the portable SKATE map package. */
final class SkateMapModel {
    private static final Logger LOGGER = LoggerFactory.getLogger("pexskate/map-model");
    private static final int MAX_MAP_BYTES = 768 * 1024 * 1024;
    private static final int MAX_VERTICES = 8_000_000;
    private static final int MAX_INDICES = 24_000_000;
    private static final long MAX_TEXTURE_PIXELS = 96_000_000L;
    private static final double DRAW_DISTANCE = 256.0;
    private static final List<Identifier> TEXTURES = new ArrayList<>();
    private static final Set<Integer> ACTIVE_MAP_SKATERS = ConcurrentHashMap.newKeySet();
    private static volatile Model model;
    private static volatile String attemptedFilename;
    private static volatile String loadingFilename;
    private static long loadGeneration;

    private SkateMapModel() {}

    static void loadDefault() {
        loadMap(null);
    }

    static boolean isReadyFor(String filename) {
        Model current = model;
        return current != null && (filename == null || filename.isBlank()
                || current.filename.equalsIgnoreCase(filename));
    }

    static boolean isLoadingFor(String filename) {
        return loadingFilename != null && filename != null && loadingFilename.equalsIgnoreCase(filename);
    }

    static void loadMap(String requestedFilename) {
        String request = requestedFilename == null || requestedFilename.isBlank() ? null : requestedFilename;
        if (isReadyFor(request)) return;
        String attemptKey = request == null ? "" : request.toLowerCase(Locale.ROOT);
        if (Objects.equals(attemptedFilename, attemptKey)) return;
        attemptedFilename = attemptKey;
        long generation = ++loadGeneration;
        loadingFilename = null;
        releaseModel();
        Minecraft client = Minecraft.getInstance();
        Path root = client.gameDirectory.toPath().resolve("config/pex-skate");
        Path maps = root.resolve("maps");
        if (!Files.isDirectory(maps)) {
            LOGGER.info("No converted Skate district maps at {}; district rendering is unavailable", maps);
            return;
        }
        try {
            Path selected = request == null ? selectMap(root, maps) : selectMapByFilename(request, maps);
            if (selected == null) {
                LOGGER.info("No converted Skate district map was found at {}", maps);
                return;
            }
            long maximumPackageBytes = Math.min(MAX_MAP_BYTES, Runtime.getRuntime().maxMemory() / 2);
            if (Files.size(selected) > maximumPackageBytes) {
                throw new IOException("map package exceeds this client's memory-based size limit");
            }
            String filename = selected.getFileName().toString();
            loadingFilename = filename;
            CompletableFuture.supplyAsync(() -> {
                try {
                    return parse(Files.readAllBytes(selected), filename);
                } catch (IOException error) {
                    throw new CompletionException(error);
                }
            }).whenComplete((parsed, error) -> client.execute(() -> {
                if (generation != loadGeneration) return;
                loadingFilename = null;
                if (error != null) {
                    releaseModel();
                    Throwable cause = error instanceof CompletionException && error.getCause() != null
                            ? error.getCause() : error;
                    LOGGER.error("Could not load the converted Skate district from {}", maps, cause);
                    return;
                }
                try {
                    model = install(client, parsed);
                    LOGGER.info("Loaded Skate district {} with {} material groups", filename, model.primitives.size());
                } catch (Exception installError) {
                    releaseModel();
                    LOGGER.error("Could not upload the converted Skate district from {}", maps, installError);
                }
            }));
        } catch (Exception error) {
            releaseModel();
            loadingFilename = null;
            LOGGER.error("Could not load the converted Skate district from {}", maps, error);
        }
    }

    private static Path selectMapByFilename(String filename, Path maps) throws IOException {
        Path requested = Path.of(filename);
        if (requested.getNameCount() != 1 || filename.contains("..") || filename.contains("/")
                || filename.contains("\\") || !filename.toLowerCase().endsWith(".skate")) {
            throw new IOException("server supplied an invalid Skate map filename");
        }
        try (var files = Files.list(maps)) {
            return files.filter(Files::isRegularFile)
                    .filter(path -> path.getFileName().toString().equalsIgnoreCase(filename))
                    .findFirst()
                    .orElseThrow(() -> new IOException("server-selected map is not installed: " + filename));
        }
    }

    private static Path selectMap(Path root, Path maps) throws IOException {
        Path setting = root.resolve("settings/default-map.txt");
        if (Files.isRegularFile(setting)) {
            String filename = Files.readString(setting, StandardCharsets.UTF_8).trim();
            if (!filename.isEmpty()) {
                if (Path.of(filename).getNameCount() != 1 || filename.contains("..") || filename.contains("/") || filename.contains("\\")) {
                    throw new IOException("config/pex-skate/settings/default-map.txt must contain one map filename");
                }
                try (var files = Files.list(maps)) {
                    return files.filter(Files::isRegularFile)
                            .filter(path -> path.getFileName().toString().equalsIgnoreCase(filename))
                            .filter(path -> path.getFileName().toString().toLowerCase().endsWith(".skate"))
                            .findFirst()
                            .orElseThrow(() -> new IOException("configured map is not installed: " + filename));
                }
            }
        }
        try (var files = Files.list(maps)) {
            List<Path> candidates = files.filter(Files::isRegularFile)
                    .filter(path -> path.getFileName().toString().toLowerCase().endsWith(".skate"))
                    .sorted((a, b) -> a.getFileName().toString().compareToIgnoreCase(b.getFileName().toString()))
                    .toList();
            return candidates.stream()
                    .filter(path -> path.getFileName().toString().equalsIgnoreCase("University.skate"))
                    .findFirst()
                    .orElse(candidates.isEmpty() ? null : candidates.getFirst());
        }
    }

    static void registerRenderer() {
        LevelRenderEvents.COLLECT_SUBMITS.register(SkateMapModel::submit);
    }

    static void acceptState(int entityId, boolean active, boolean mapBacked) {
        if (active && mapBacked) ACTIVE_MAP_SKATERS.add(entityId);
        else ACTIVE_MAP_SKATERS.remove(entityId);
    }

    static void clearActive() {
        ACTIVE_MAP_SKATERS.clear();
        if (model == null) attemptedFilename = null;
    }

    static boolean isReady() { return model != null; }

    private static void submit(LevelRenderContext context) {
        Model current = model;
        Minecraft client = Minecraft.getInstance();
        if (current == null || ACTIVE_MAP_SKATERS.isEmpty() || client.level == null) return;
        Vec3 camera = context.gameRenderer().mainCamera().position();
        PoseStack matrices = new PoseStack();
        matrices.translate(-camera.x, -camera.y, -camera.z);
        for (Primitive primitive : current.primitives) {
            if (primitive.distanceSquared(camera) > DRAW_DISTANCE * DRAW_DISTANCE) continue;
            BlockPos lightPosition = primitive.lightSamplePosition();
            int light = client.level.getBrightness(LightLayer.SKY, lightPosition) << 20
                    | client.level.getBrightness(LightLayer.BLOCK, lightPosition) << 4;
            context.submitNodeCollector().submitCustomGeometry(
                    matrices, RenderTypes.entityCutout(primitive.texture),
                    (pose, vertices) -> primitive.draw(pose, vertices, light));
        }
    }

    static void close() {
        loadGeneration++;
        loadingFilename = null;
        releaseModel();
        ACTIVE_MAP_SKATERS.clear();
        attemptedFilename = null;
    }

    private static void releaseModel() {
        Minecraft client = Minecraft.getInstance();
        for (Identifier texture : TEXTURES) client.getTextureManager().release(texture);
        TEXTURES.clear();
        model = null;
    }

    private static ParsedModel parse(byte[] file, String filename) throws IOException {
        Reader reader = new Reader(file);
        byte[] magic = reader.take(8);
        if (magic[0] != 'S' || magic[1] != 'K' || magic[2] != 'A' || magic[3] != 'T' || magic[4] != 'E'
                || magic[5] != '1' || magic[6] != '4' || magic[7] != 0) {
            throw new IOException("only SKATE14 retail map packages are supported by this renderer");
        }
        if (reader.u32() != 0x12345678L) throw new IOException("invalid SKATE endian marker");
        String mapName = reader.string();
        reader.skipFloats(3 + 1 + 45);
        long materialCountLong = reader.u32();
        long textureCountLong = reader.u32();
        long vertexCountLong = reader.u32();
        long indexCountLong = reader.u32();
        long collisionCountLong = reader.u32();
        long railCount = reader.u32();
        long doorCount = reader.u32();
        long lightCount = reader.u32();
        long routeCount = reader.u32();
        if (materialCountLong == 0 || materialCountLong > 65_536 || textureCountLong > 65_536
                || vertexCountLong == 0 || vertexCountLong > MAX_VERTICES || indexCountLong == 0
                || indexCountLong > MAX_INDICES || indexCountLong % 3 != 0
                || collisionCountLong > MAX_INDICES / 3
                || railCount > 1_000_000 || doorCount > 1_000_000 || lightCount > 1_000_000
                || routeCount > 1_000_000) {
            throw new IOException("invalid SKATE district geometry counts");
        }
        int materialCount = (int) materialCountLong;
        int textureCount = (int) textureCountLong;
        int vertexCount = (int) vertexCountLong;
        int indexCount = (int) indexCountLong;
        long geometryMemoryEstimate = file.length
                + vertexCountLong * 92L
                + indexCountLong * 8L;
        long parserBudget = Runtime.getRuntime().maxMemory() / 2;
        if (geometryMemoryEstimate > parserBudget) {
            throw new IOException("SKATE district geometry exceeds this client's parse memory budget");
        }
        long[] materialTextures = new long[materialCount];
        float[][] materialColors = new float[materialCount][4];
        for (int i = 0; i < materialCount; i++) {
            reader.string();
            reader.u32(); // flags
            reader.skipFloats(2);
            materialColors[i][0] = reader.f32();
            materialColors[i][1] = reader.f32();
            materialColors[i][2] = reader.f32();
            materialColors[i][3] = 1.0f;
            reader.skipFloats(2); // roughness and emissive
            materialTextures[i] = reader.u32(); // albedo texture, one-based
            reader.skipU32(1); // indirect texture
            reader.skipU32(3); // normal, ORM, emissive
            reader.skipFloats(1); // indirect strength
            reader.skipU32(1); // alpha mode
            reader.skipFloats(1); // alpha cutoff
            reader.skipU32(3); // audio, physics, pattern
            reader.skipU32(1); // depth layer
            if (reader.u32() != 0) skipRetailDefinition(reader, textureCount);
        }

        List<TextureData> textures = new ArrayList<>(textureCount);
        long texturePixels = 0;
        for (int i = 0; i < textureCount; i++) {
            String textureName = reader.string();
            int width = checkedInt(reader.u32(), 16_384, "texture width");
            int height = checkedInt(reader.u32(), 16_384, "texture height");
            reader.u32(); // color space
            long pixels = (long) width * height;
            texturePixels += pixels;
            if (width == 0 || height == 0 || texturePixels > MAX_TEXTURE_PIXELS
                    || geometryMemoryEstimate + texturePixels * 4L > parserBudget) {
                throw new IOException("SKATE district textures exceed client memory limit");
            }
            byte[] rgba = reader.stored(Math.multiplyExact(Math.multiplyExact(width, height), 4));
            textures.add(new TextureData(textureName, width, height, rgba));
        }

        byte[] vertexBytes = reader.stored(Math.multiplyExact(vertexCount, 56));
        byte[] indexBytes = reader.stored(Math.multiplyExact(indexCount, 4));
        reader.stored(Math.multiplyExact(Math.toIntExact(collisionCountLong), 48));
        ByteBuffer vertices = ByteBuffer.wrap(vertexBytes).order(ByteOrder.LITTLE_ENDIAN);
        float[] positions = new float[vertexCount * 3];
        float[] normals = new float[vertexCount * 3];
        float[] uv = new float[vertexCount * 2];
        int[] vertexMaterials = new int[vertexCount];
        for (int i = 0; i < vertexCount; i++) {
            int p = i * 3;
            for (int axis = 0; axis < 3; axis++) {
                float value = vertices.getFloat();
                if (!Float.isFinite(value)) throw new IOException("non-finite SKATE map vertex");
                positions[p + axis] = value;
            }
            for (int axis = 0; axis < 3; axis++) {
                float value = vertices.getFloat();
                if (!Float.isFinite(value)) throw new IOException("non-finite SKATE map normal");
                normals[p + axis] = value;
            }
            int uvAt = i * 2;
            uv[uvAt] = vertices.getFloat();
            uv[uvAt + 1] = vertices.getFloat();
            if (!Float.isFinite(uv[uvAt]) || !Float.isFinite(uv[uvAt + 1])) {
                throw new IOException("non-finite SKATE map texture coordinate");
            }
            vertices.position(vertices.position() + 8); // lightmap UV
            long material = Integer.toUnsignedLong(vertices.getInt()) - 1;
            if (material < 0 || material >= materialCount) throw new IOException("SKATE vertex references an invalid material");
            vertexMaterials[i] = (int) material;
            vertices.position(vertices.position() + 12); // decal UV and tangent frame
        }
        ByteBuffer indexBuffer = ByteBuffer.wrap(indexBytes).order(ByteOrder.LITTLE_ENDIAN);
        Map<ChunkMaterial, IntBuilder> grouped = new HashMap<>();
        for (int i = 0; i < indexCount; i += 3) {
            int a = checkedIndex(indexBuffer.getInt(), vertexCount);
            int b = checkedIndex(indexBuffer.getInt(), vertexCount);
            int c = checkedIndex(indexBuffer.getInt(), vertexCount);
            int material = vertexMaterials[a];
            if (vertexMaterials[b] != material || vertexMaterials[c] != material) {
                throw new IOException("SKATE triangle has mixed material indices");
            }
            float centerX = (positions[a * 3] + positions[b * 3] + positions[c * 3]) / 3.0f;
            float centerZ = (positions[a * 3 + 2] + positions[b * 3 + 2] + positions[c * 3 + 2]) / 3.0f;
            ChunkMaterial key = new ChunkMaterial(material,
                    (int) Math.floor(centerX / 64.0f), (int) Math.floor(centerZ / 64.0f));
            IntBuilder indices = grouped.computeIfAbsent(key, ignored -> new IntBuilder());
            indices.add(a);
            indices.add(b);
            indices.add(c);
        }
        List<PendingPrimitive> primitives = new ArrayList<>();
        for (Map.Entry<ChunkMaterial, IntBuilder> entry : grouped.entrySet()) {
            int material = entry.getKey().material;
            int texture = Math.toIntExact(materialTextures[material]);
            if (texture < 0 || texture > textures.size()) throw new IOException("SKATE material references an invalid texture");
            int[] materialIndices = entry.getValue().toArray();
            primitives.add(new PendingPrimitive(positions, normals, uv, materialIndices, texture, materialColors[material]));
        }
        return new ParsedModel(filename, mapName, List.copyOf(textures), List.copyOf(primitives));
    }

    private static Model install(Minecraft client, ParsedModel parsed) {
        Identifier[] textures = new Identifier[parsed.textures.size() + 1];
        textures[0] = registerWhite(client, parsed.filename);
        for (int i = 0; i < parsed.textures.size(); i++) {
            TextureData texture = parsed.textures.get(i);
            NativeImage image = new NativeImage(texture.width, texture.height, false);
            boolean transferred = false;
            try {
                image.getPixelBytes().put(texture.rgba);
                Identifier id = Identifier.fromNamespaceAndPath(
                        "pexskate", "map/" + safePath(parsed.filename) + "/texture_" + i);
                client.getTextureManager().register(id, new DynamicTexture(
                        () -> "PEX Skate " + parsed.mapName + " " + texture.name, image));
                TEXTURES.add(id);
                textures[i + 1] = id;
                transferred = true;
            } finally {
                if (!transferred) image.close();
            }
        }
        List<Primitive> primitives = new ArrayList<>(parsed.primitives.size());
        for (PendingPrimitive primitive : parsed.primitives) {
            primitives.add(new Primitive(primitive.positions, primitive.normals, primitive.uv,
                    primitive.indices, textures[primitive.textureIndex], primitive.color));
        }
        return new Model(parsed.filename, List.copyOf(primitives));
    }

    private static void skipRetailDefinition(Reader reader, int textureCount) throws IOException {
        reader.skip(16);
        reader.string();
        reader.skip(8);
        int bindings = checkedInt(reader.u32(), 65_536, "retail texture binding count");
        for (int i = 0; i < bindings; i++) {
            reader.string();
            long texture = reader.u32();
            if (texture > textureCount) throw new IOException("retail material references an invalid texture");
            reader.skipU32(3);
        }
        int parameters = checkedInt(reader.u32(), 65_536, "retail material parameter count");
        for (int i = 0; i < parameters; i++) {
            reader.string();
            int values = checkedInt(reader.u32(), 65_536, "retail parameter value count");
            for (int value = 0; value < values; value++) reader.string();
        }
        reader.string();
    }

    private static Identifier registerWhite(Minecraft client, String filename) {
        Identifier id = Identifier.fromNamespaceAndPath("pexskate", "map/" + safePath(filename) + "/white");
        NativeImage pixel = new NativeImage(1, 1, false);
        pixel.setPixel(0, 0, 0xffffffff);
        client.getTextureManager().register(id, new DynamicTexture(() -> "PEX Skate map fallback", pixel));
        TEXTURES.add(id);
        return id;
    }

    private static String safePath(String value) {
        return value.toLowerCase().replaceAll("[^a-z0-9._/-]", "_").replace('/', '_').replace('\\', '_');
    }

    private static int checkedIndex(int index, int vertexCount) throws IOException {
        long value = Integer.toUnsignedLong(index);
        if (value >= vertexCount) throw new IOException("SKATE map index exceeds vertex count");
        return (int) value;
    }

    private static int checkedInt(long value, int maximum, String label) throws IOException {
        if (value < 0 || value > maximum) throw new IOException("invalid " + label);
        return (int) value;
    }

    private record Model(String filename, List<Primitive> primitives) {}
    private record ParsedModel(String filename, String mapName, List<TextureData> textures,
                               List<PendingPrimitive> primitives) {}
    private record TextureData(String name, int width, int height, byte[] rgba) {}
    private record PendingPrimitive(float[] positions, float[] normals, float[] uv, int[] indices,
                                    int textureIndex, float[] color) {}
    private record ChunkMaterial(int material, int x, int z) {}

    private static final class IntBuilder {
        private int[] values = new int[768];
        private int size;

        private void add(int value) {
            if (size == values.length) values = java.util.Arrays.copyOf(values, Math.multiplyExact(size, 2));
            values[size++] = value;
        }

        private int[] toArray() { return java.util.Arrays.copyOf(values, size); }
    }

    private static final class Primitive {
        private final float[] positions, normals, uv, color;
        private final int[] indices;
        private final Identifier texture;
        private final float[] bounds;

        private Primitive(float[] positions, float[] normals, float[] uv, int[] indices,
                          Identifier texture, float[] color) {
            this.positions = positions;
            this.normals = normals;
            this.uv = uv;
            this.indices = indices;
            this.texture = texture;
            this.color = color.clone();
            this.bounds = new float[] {Float.POSITIVE_INFINITY, Float.POSITIVE_INFINITY, Float.POSITIVE_INFINITY,
                    Float.NEGATIVE_INFINITY, Float.NEGATIVE_INFINITY, Float.NEGATIVE_INFINITY};
            for (int index : indices) {
                int point = index * 3;
                for (int axis = 0; axis < 3; axis++) {
                    this.bounds[axis] = Math.min(this.bounds[axis], positions[point + axis]);
                    this.bounds[axis + 3] = Math.max(this.bounds[axis + 3], positions[point + axis]);
                }
            }
        }

        private double distanceSquared(Vec3 point) {
            double distance = 0.0;
            for (int axis = 0; axis < 3; axis++) {
                double value = axis == 0 ? point.x : axis == 1 ? point.y : point.z;
                double delta = value < bounds[axis] ? bounds[axis] - value
                        : value > bounds[axis + 3] ? value - bounds[axis + 3] : 0.0;
                distance += delta * delta;
            }
            return distance;
        }

        private BlockPos lightSamplePosition() {
            return BlockPos.containing(
                    (bounds[0] + bounds[3]) * 0.5,
                    (bounds[1] + bounds[4]) * 0.5,
                    (bounds[2] + bounds[5]) * 0.5);
        }

        private void draw(PoseStack.Pose pose, VertexConsumer output, int light) {
            Vector3f normal = new Vector3f();
            for (int index : indices) {
                int point = index * 3;
                int texCoord = index * 2;
                normal.set(normals[point], normals[point + 1], normals[point + 2]);
                pose.normal().transform(normal).normalize();
                output.addVertex(pose, positions[point], positions[point + 1], positions[point + 2])
                        .setColor(color[0], color[1], color[2], color[3])
                        .setUv(uv[texCoord], uv[texCoord + 1])
                        .setLight(light)
                        .setNormal(normal.x, normal.y, normal.z);
            }
        }
    }

    private static final class Reader {
        private final ByteBuffer data;

        private Reader(byte[] bytes) { data = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN); }

        private byte[] take(int length) throws IOException {
            if (length < 0 || length > data.remaining()) throw new IOException("truncated SKATE map package");
            byte[] bytes = new byte[length];
            data.get(bytes);
            return bytes;
        }

        private void skip(int length) throws IOException {
            if (length < 0 || length > data.remaining()) throw new IOException("truncated SKATE map package");
            data.position(data.position() + length);
        }

        private long u32() throws IOException {
            if (data.remaining() < 4) throw new IOException("truncated SKATE integer");
            return Integer.toUnsignedLong(data.getInt());
        }

        private float f32() throws IOException {
            if (data.remaining() < 4) throw new IOException("truncated SKATE float");
            float value = data.getFloat();
            if (!Float.isFinite(value)) throw new IOException("non-finite SKATE value");
            return value;
        }

        private void skipU32(int count) throws IOException { skip(Math.multiplyExact(count, 4)); }
        private void skipFloats(int count) throws IOException { skipU32(count); }

        private String string() throws IOException {
            int length = checkedInt(u32(), 1_048_576, "string length");
            return new String(take(length), StandardCharsets.UTF_8);
        }

        private byte[] stored(int expected) throws IOException {
            long method = u32();
            int length = checkedInt(u32(), data.remaining(), "stored block length");
            byte[] compressed = take(length);
            if (method == 0) {
                if (compressed.length != expected) throw new IOException("SKATE raw block length mismatch");
                return compressed;
            }
            if (method != 1) throw new IOException("unsupported SKATE storage method " + method);
            try (InflaterInputStream input = new InflaterInputStream(new ByteArrayInputStream(compressed));
                 ByteArrayOutputStream output = new ByteArrayOutputStream(expected)) {
                byte[] buffer = new byte[64 * 1024];
                int total = 0;
                int count;
                while ((count = input.read(buffer)) != -1) {
                    total = Math.addExact(total, count);
                    if (total > expected) throw new IOException("SKATE decompressed block exceeds its declared size");
                    output.write(buffer, 0, count);
                }
                if (total != expected) throw new IOException("SKATE decompressed block length mismatch");
                return output.toByteArray();
            } catch (ArithmeticException error) {
                throw new IOException("SKATE block size overflow", error);
            }
        }
    }
}
