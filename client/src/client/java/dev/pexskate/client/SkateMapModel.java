package dev.pexskate.client;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
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
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HexFormat;
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
public final class SkateMapModel {
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
    private static volatile UploadTask uploadTask;
    private static long loadGeneration;

    private SkateMapModel() {}

    static void loadDefault() {
        loadMap(null);
    }

    static boolean isReadyFor(String filename) {
        return isReadyFor(filename, "", "");
    }

    static boolean isReadyFor(String filename, String mapSha256, String propsSha256) {
        Model current = model;
        return current != null && (filename == null || filename.isBlank()
                || current.filename.equalsIgnoreCase(filename))
                && (mapSha256 == null || mapSha256.isBlank() || current.mapSha256.equalsIgnoreCase(mapSha256))
                && (propsSha256 == null || (mapSha256 == null || mapSha256.isBlank()
                        ? propsSha256.isBlank() || current.propsSha256.equalsIgnoreCase(propsSha256)
                        : current.propsSha256.equalsIgnoreCase(propsSha256)));
    }

    static boolean isLoadingFor(String filename) {
        return loadingFilename != null && filename != null && loadingFilename.equalsIgnoreCase(filename);
    }

    static void loadMap(String requestedFilename) {
        loadMap(requestedFilename, "", "");
    }

    static void loadMap(String requestedFilename, String expectedMapSha256, String expectedPropsSha256) {
        String request = requestedFilename == null || requestedFilename.isBlank() ? null : requestedFilename;
        final String requiredMapSha256 = expectedMapSha256 == null ? "" : expectedMapSha256;
        final String requiredPropsSha256 = expectedPropsSha256 == null ? "" : expectedPropsSha256;
        if (isReadyFor(request, requiredMapSha256, requiredPropsSha256)) return;
        String attemptKey = (request == null ? "" : request.toLowerCase(Locale.ROOT)) + ":" + requiredMapSha256 + ":" + requiredPropsSha256;
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
                    byte[] file = Files.readAllBytes(selected);
                    verifyCatalog(root, filename, file);
                    String actualMapSha256 = sha256(file);
                    if (!requiredMapSha256.isBlank() && !actualMapSha256.equalsIgnoreCase(requiredMapSha256)) {
                        throw new IOException("server and client converted Skate map checksums differ for " + filename);
                    }
                    ParsedModel district = parse(file, filename);
                    Path propsPath = root.resolve("props").resolve(filename);
                    if (!requiredPropsSha256.isBlank()) {
                        try {
                            if (!Files.isRegularFile(propsPath)) throw new IOException("server requires authored props, but this client has no " + propsPath);
                            long propsLimit = Math.min(MAX_MAP_BYTES, Runtime.getRuntime().maxMemory() / 2);
                            if (Files.size(propsPath) > propsLimit) {
                                throw new IOException("authored district prop package exceeds this client's size limit");
                            }
                            byte[] propsFile = Files.readAllBytes(propsPath);
                            if (!sha256(propsFile).equalsIgnoreCase(requiredPropsSha256)) {
                                throw new IOException("server and client converted Skate prop checksums differ for " + filename);
                            }
                            verifyPropsCatalog(root, filename, propsFile);
                            ParsedModel props = parse(propsFile, filename + "-props");
                            district = mergeProps(district, props);
                        } catch (IOException | RuntimeException error) {
                            throw new CompletionException(error);
                        }
                    } else if (Files.isRegularFile(propsPath)) {
                        LOGGER.info("Skipping local authored props because the server map has no prop package for {}", filename);
                    }
                    return district;
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
                    beginUpload(client, parsed, generation);
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

    private static void verifyCatalog(Path root, String filename, byte[] file) throws IOException {
        Path catalogPath = root.resolve("maps.json");
        if (!Files.isRegularFile(catalogPath)) {
            // Older/manual installations may only have the .skate files. New
            // setup-helper installs include the catalog and get integrity checks.
            LOGGER.info("No converted Skate map catalog at {}; skipping district checksum validation", catalogPath);
            return;
        }
        JsonElement document;
        try (var reader = Files.newBufferedReader(catalogPath, StandardCharsets.UTF_8)) {
            document = JsonParser.parseReader(reader);
        } catch (RuntimeException error) {
            throw new IOException("converted Skate map catalog is not valid JSON", error);
        }
        if (document == null || !document.isJsonArray()) {
            throw new IOException("converted Skate map catalog must be a JSON array");
        }
        String expectedPath = "maps/" + filename;
        String expectedDigest = null;
        JsonArray entries = document.getAsJsonArray();
        for (JsonElement item : entries) {
            if (!item.isJsonObject()) continue;
            JsonObject entry = item.getAsJsonObject();
            if (!entry.has("path") || !entry.get("path").isJsonPrimitive()
                    || !expectedPath.equals(entry.get("path").getAsString())) continue;
            if (expectedDigest != null) {
                throw new IOException("converted Skate map catalog contains duplicate entries for " + filename);
            }
            if (!entry.has("sha256") || !entry.get("sha256").isJsonPrimitive()) {
                throw new IOException("converted Skate map catalog has no SHA-256 for " + filename);
            }
            expectedDigest = entry.get("sha256").getAsString();
        }
        if (expectedDigest == null || !expectedDigest.matches("(?i)[0-9a-f]{64}")) {
            throw new IOException("converted Skate map catalog has no valid SHA-256 entry for " + filename);
        }
        try {
            String actualDigest = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(file));
            if (!actualDigest.equalsIgnoreCase(expectedDigest)) {
                throw new IOException("converted Skate map checksum does not match maps.json for " + filename);
            }
        } catch (NoSuchAlgorithmException error) {
            throw new IOException("SHA-256 is unavailable on this Java runtime", error);
        }
    }

    private static String sha256(byte[] file) throws IOException {
        try {
            return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(file));
        } catch (NoSuchAlgorithmException error) {
            throw new IOException("SHA-256 is unavailable on this Java runtime", error);
        }
    }

    private static void verifyPropsCatalog(Path root, String filename, byte[] file) throws IOException {
        Path catalogPath = root.resolve("maps.json");
        if (!Files.isRegularFile(catalogPath)) {
            LOGGER.info("No converted Skate map catalog at {}; skipping district prop checksum validation", catalogPath);
            return;
        }
        JsonElement document;
        try (var reader = Files.newBufferedReader(catalogPath, StandardCharsets.UTF_8)) {
            document = JsonParser.parseReader(reader);
        } catch (RuntimeException error) {
            throw new IOException("converted Skate map catalog is not valid JSON", error);
        }
        if (document == null || !document.isJsonArray()) {
            throw new IOException("converted Skate map catalog must be a JSON array");
        }
        String expectedPath = "maps/" + filename;
        JsonObject selected = null;
        for (JsonElement item : document.getAsJsonArray()) {
            if (!item.isJsonObject()) continue;
            JsonObject entry = item.getAsJsonObject();
            if (entry.has("path") && entry.get("path").isJsonPrimitive()
                    && expectedPath.equals(entry.get("path").getAsString())) {
                if (selected != null) throw new IOException("converted Skate map catalog contains duplicate entries for " + filename);
                selected = entry;
            }
        }
        if (selected == null) throw new IOException("converted Skate map catalog has no entry for " + filename);
        if (!selected.has("props")) {
            LOGGER.info("Map catalog has no authored prop checksum for {}; allowing legacy prop package", filename);
            return;
        }
        JsonElement propsElement = selected.get("props");
        if (!propsElement.isJsonObject()) throw new IOException("converted Skate prop catalog entry is invalid");
        JsonObject props = propsElement.getAsJsonObject();
        String expectedPropsPath = "props/" + filename;
        if (!props.has("path") || !props.get("path").isJsonPrimitive()
                || !expectedPropsPath.equals(props.get("path").getAsString())
                || !props.has("sha256") || !props.get("sha256").isJsonPrimitive()) {
            throw new IOException("converted Skate prop catalog entry does not match " + filename);
        }
        String expectedDigest = props.get("sha256").getAsString();
        if (!expectedDigest.matches("(?i)[0-9a-f]{64}")) {
            throw new IOException("converted Skate prop catalog has an invalid SHA-256 for " + filename);
        }
        try {
            String actualDigest = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(file));
            if (!actualDigest.equalsIgnoreCase(expectedDigest)) {
                throw new IOException("converted Skate prop checksum does not match maps.json for " + filename);
            }
        } catch (NoSuchAlgorithmException error) {
            throw new IOException("SHA-256 is unavailable on this Java runtime", error);
        }
    }

    private static ParsedModel mergeProps(ParsedModel district, ParsedModel props) throws IOException {
        if (props.sky != null) throw new IOException("authored district prop package unexpectedly contains a sky");
        long memoryEstimate = district.memoryEstimate + props.memoryEstimate;
        if (memoryEstimate > Runtime.getRuntime().maxMemory() / 2) {
            throw new IOException("district geometry and authored props exceed this client's combined memory limit");
        }
        int textureOffset = district.textures.size();
        List<TextureData> textures = new ArrayList<>(district.textures.size() + props.textures.size());
        textures.addAll(district.textures);
        textures.addAll(props.textures);
        List<PendingPrimitive> primitives = new ArrayList<>(district.primitives.size() + props.primitives.size());
        primitives.addAll(district.primitives);
        for (PendingPrimitive primitive : props.primitives) {
            int texture = primitive.textureIndex == 0 ? 0 : primitive.textureIndex + textureOffset;
            primitives.add(new PendingPrimitive(primitive.positions, primitive.normals, primitive.uv,
                    primitive.vertexIndirectLight, primitive.indices, primitive.bounds, texture, primitive.color,
                    primitive.alphaMode));
        }
        return new ParsedModel(district.filename, district.mapName, district.mapSha256, props.mapSha256,
                List.copyOf(textures), List.copyOf(primitives), district.sky, memoryEstimate);
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

    public static FogSettings localFogSettings() {
        Minecraft client = Minecraft.getInstance();
        if (client.player == null || !ACTIVE_MAP_SKATERS.contains(client.player.getId())) return null;
        Model current = model;
        return current == null || current.sky == null ? null : current.sky.fogSettings();
    }

    static void tick(Minecraft client) {
        UploadTask task = uploadTask;
        if (task == null) return;
        if (task.generation != loadGeneration) {
            uploadTask = null;
            return;
        }
        try {
            if (task.nextTexture < task.parsed.textures.size()) {
                uploadTexture(client, task, task.nextTexture++);
                return;
            }
            List<Primitive> primitives = new ArrayList<>(task.parsed.primitives.size());
            for (PendingPrimitive primitive : task.parsed.primitives) {
                primitives.add(new Primitive(primitive.positions, primitive.normals, primitive.uv,
                        primitive.vertexIndirectLight, primitive.indices, primitive.bounds,
                        task.textureIds[primitive.textureIndex], primitive.color, primitive.alphaMode));
            }
            PendingSky pendingSky = task.parsed.sky;
            Sky sky = pendingSky == null ? null : new Sky(pendingSky, task.textureIds[pendingSky.diffuseTextureIndex]);
            model = new Model(task.parsed.filename, task.parsed.mapSha256, task.parsed.propsSha256,
                    List.copyOf(primitives), sky);
            uploadTask = null;
            loadingFilename = null;
            LOGGER.info("Loaded Skate district {} with {} material groups",
                    task.parsed.filename, model.primitives.size());
        } catch (Exception installError) {
            uploadTask = null;
            loadingFilename = null;
            releaseModel();
            LOGGER.error("Could not upload the converted Skate district", installError);
        }
    }

    private static void submit(LevelRenderContext context) {
        Model current = model;
        Minecraft client = Minecraft.getInstance();
        if (current == null || ACTIVE_MAP_SKATERS.isEmpty() || client.level == null) return;
        Vec3 camera = context.gameRenderer().mainCamera().position();
        PoseStack matrices = new PoseStack();
        matrices.translate(-camera.x, -camera.y, -camera.z);
        if (current.sky != null) {
            Sky sky = current.sky;
            context.submitNodeCollector().submitCustomGeometry(
                    matrices, RenderTypes.entityTranslucent(sky.diffuseTexture),
                    (pose, vertices) -> sky.draw(pose, vertices, camera));
        }
        List<Primitive> translucent = new ArrayList<>();
        for (Primitive primitive : current.primitives) {
            if (primitive.distanceSquared(camera) > DRAW_DISTANCE * DRAW_DISTANCE) continue;
            if (primitive.alphaMode == 2) translucent.add(primitive);
            else submitPrimitive(context, matrices, client, primitive, false);
        }
        translucent.sort(Comparator.comparingDouble(
                (Primitive primitive) -> primitive.centerDistanceSquared(camera)).reversed());
        for (Primitive primitive : translucent) submitPrimitive(context, matrices, client, primitive, true);
    }

    private static void submitPrimitive(LevelRenderContext context, PoseStack matrices, Minecraft client,
                                        Primitive primitive, boolean translucent) {
        BlockPos lightPosition = primitive.lightSamplePosition();
        int light = client.level.getBrightness(LightLayer.SKY, lightPosition) << 20
                | client.level.getBrightness(LightLayer.BLOCK, lightPosition) << 4;
        context.submitNodeCollector().submitCustomGeometry(
                matrices, translucent ? RenderTypes.entityTranslucent(primitive.texture)
                        : RenderTypes.entityCutout(primitive.texture),
                (pose, vertices) -> primitive.draw(pose, vertices, light));
    }

    static void close() {
        loadGeneration++;
        loadingFilename = null;
        releaseModel();
        ACTIVE_MAP_SKATERS.clear();
        attemptedFilename = null;
    }

    private static void releaseModel() {
        uploadTask = null;
        Minecraft client = Minecraft.getInstance();
        for (Identifier texture : TEXTURES) client.getTextureManager().release(texture);
        TEXTURES.clear();
        model = null;
    }

    private static ParsedModel parse(byte[] file, String filename) throws IOException {
        String mapSha256 = sha256(file);
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
                + vertexCountLong * 104L
                + indexCountLong * 8L;
        long parserBudget = Runtime.getRuntime().maxMemory() / 2;
        if (geometryMemoryEstimate > parserBudget) {
            throw new IOException("SKATE district geometry exceeds this client's parse memory budget");
        }
        long[] materialTextures = new long[materialCount];
        long[] materialLightmaps = new long[materialCount];
        int[] materialAlphaModes = new int[materialCount];
        float[] materialLightmapStrengths = new float[materialCount];
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
            materialLightmaps[i] = reader.u32(); // baked indirect-light texture, one-based
            reader.skipU32(3); // normal, ORM, emissive
            materialLightmapStrengths[i] = Math.clamp(reader.f32(), 0.0f, 1.0f);
            long alphaMode = reader.u32();
            if (alphaMode > 2) throw new IOException("SKATE material has an unsupported alpha mode");
            materialAlphaModes[i] = (int) alphaMode;
            reader.skipFloats(1); // alpha cutoff
            reader.skipU32(3); // audio, physics, pattern
            reader.skipU32(1); // depth layer
            if (reader.u32() != 0) skipRetailDefinition(reader, textureCount);
        }
        for (long lightmap : materialLightmaps) {
            if (lightmap > textureCount) throw new IOException("SKATE material references an invalid lightmap");
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
        float[] vertexIndirectLight = new float[vertexCount * 3];
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
            float lightU = vertices.getFloat();
            float lightV = vertices.getFloat();
            if (!Float.isFinite(lightU) || !Float.isFinite(lightV)) {
                throw new IOException("non-finite SKATE lightmap coordinate");
            }
            long material = Integer.toUnsignedLong(vertices.getInt()) - 1;
            if (material < 0 || material >= materialCount) throw new IOException("SKATE vertex references an invalid material");
            vertexMaterials[i] = (int) material;
            sampleIndirectLight(textures, materialLightmaps[(int) material],
                    materialLightmapStrengths[(int) material], lightU, lightV,
                    vertexIndirectLight, p);
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
            primitives.add(new PendingPrimitive(positions, normals, uv, vertexIndirectLight,
                    materialIndices, primitiveBounds(positions, materialIndices), texture, materialColors[material],
                    materialAlphaModes[material]));
        }
        skipRails(reader, railCount);
        if (doorCount != 0 || lightCount != 0 || routeCount != 0) {
            throw new IOException("district doors, lights, and NPC routes are not supported by the Fabric renderer yet");
        }
        PendingSky sky = null;
        int extensionCount = checkedInt(reader.u32(), 20, "map extension count");
        for (int extension = 0; extension < extensionCount; extension++) {
            byte[] tag = reader.take(4);
            long schema = reader.u32();
            int decodedSize = checkedInt(reader.u32(), MAX_MAP_BYTES, "map extension size");
            if (tag[0] == 'S' && tag[1] == 'K' && tag[2] == 'Y' && tag[3] == 'B' && schema == 2) {
                if (sky != null) throw new IOException("SKATE map contains more than one client sky extension");
                if (geometryMemoryEstimate + texturePixels * 4L + decodedSize * 2L > parserBudget) {
                    throw new IOException("SKATE sky extension exceeds this client's parse memory budget");
                }
                sky = parseSky(reader.stored(decodedSize), textures, geometryMemoryEstimate, texturePixels);
            } else {
                reader.skipStored(decodedSize);
            }
        }
        if (reader.remaining() != 0) throw new IOException("SKATE map has unexplained trailing bytes");
        if (sky != null && (texturePixels + sky.texturePixels > MAX_TEXTURE_PIXELS
                || geometryMemoryEstimate + (texturePixels + sky.texturePixels) * 4L
                + sky.positions.length * 4L + sky.indices.length * 4L > parserBudget)) {
            throw new IOException("SKATE authored sky exceeds this client's memory limit");
        }
        long parsedMemoryEstimate = geometryMemoryEstimate + (texturePixels + (sky == null ? 0 : sky.texturePixels)) * 4L;
        if (sky != null) parsedMemoryEstimate += sky.positions.length * 4L + sky.indices.length * 4L;
        return new ParsedModel(filename, mapName, mapSha256, "", List.copyOf(textures), List.copyOf(primitives), sky, parsedMemoryEstimate);
    }

    private static void skipRails(Reader reader, long count) throws IOException {
        for (long i = 0; i < count; i++) {
            reader.string();
            reader.u32(); // closed
            long representation = reader.u32();
            if (representation == 0) {
                int points = checkedInt(reader.u32(), 1_000_000, "rail point count");
                reader.skip(Math.multiplyExact(points, 12));
            } else if (representation == 1) {
                reader.skip(24); // spline id, type signature, flags, trailing word
                int segments = checkedInt(reader.u32(), 1_000_000, "rail segment count");
                reader.skip(Math.multiplyExact(segments, 120));
            } else {
                throw new IOException("unsupported SKATE rail representation " + representation);
            }
        }
    }

    private static PendingSky parseSky(byte[] payload, List<TextureData> textures,
                                       long geometryMemoryEstimate, long existingTexturePixels) throws IOException {
        Reader reader = new Reader(payload);
        int vertexCount = checkedInt(reader.u32(), 2_000_000, "sky vertex count");
        int indexCount = checkedInt(reader.u32(), 6_000_000, "sky index count");
        int width = checkedInt(reader.u32(), 16_384, "sky texture width");
        int height = checkedInt(reader.u32(), 16_384, "sky texture height");
        int sunWidth = checkedInt(reader.u32(), 16_384, "sky sun texture width");
        int sunHeight = checkedInt(reader.u32(), 16_384, "sky sun texture height");
        if (vertexCount == 0 || indexCount == 0 || indexCount % 3 != 0
                || width == 0 || height == 0 || sunWidth == 0 || sunHeight == 0
                || (long) width * height > MAX_TEXTURE_PIXELS
                || (long) sunWidth * sunHeight > MAX_TEXTURE_PIXELS) {
            throw new IOException("invalid SKATE authored sky dimensions");
        }
        long skyPixels = (long) width * height + (long) sunWidth * sunHeight;
        long parseBudget = Runtime.getRuntime().maxMemory() / 2;
        if (existingTexturePixels + skyPixels > MAX_TEXTURE_PIXELS
                || geometryMemoryEstimate + (existingTexturePixels + skyPixels) * 4L
                + vertexCount * 24L + indexCount * 4L > parseBudget) {
            throw new IOException("SKATE authored sky exceeds this client's memory limit");
        }
        float fogNear = reader.f32(), fogFar = reader.f32();
        float fogPower = reader.f32(), fogMaximum = reader.f32();
        float[] fogColor = {reader.f32(), reader.f32(), reader.f32(), reader.f32()};
        float[] sunDirection = {reader.f32(), reader.f32(), reader.f32()};
        float sunScale = reader.f32(), exposure = reader.f32();
        if (fogNear < 0 || fogFar <= fogNear || fogPower <= 0 || fogMaximum < 0 || fogMaximum > 1
                || sunScale <= 0 || exposure < 0) {
            throw new IOException("invalid SKATE authored sky environment values");
        }
        byte[] diffuse = reader.stored(Math.multiplyExact(Math.multiplyExact(width, height), 4));
        byte[] sun = reader.stored(Math.multiplyExact(Math.multiplyExact(sunWidth, sunHeight), 4));
        ByteBuffer vertexData = ByteBuffer.wrap(reader.stored(Math.multiplyExact(vertexCount, 20)))
                .order(ByteOrder.LITTLE_ENDIAN);
        float[] positions = new float[vertexCount * 3];
        float[] uv = new float[vertexCount * 2];
        for (int vertex = 0; vertex < vertexCount; vertex++) {
            int p = vertex * 3, t = vertex * 2;
            for (int axis = 0; axis < 3; axis++) {
                float value = vertexData.getFloat();
                if (!Float.isFinite(value)) throw new IOException("non-finite SKATE sky vertex");
                positions[p + axis] = value;
            }
            uv[t] = vertexData.getFloat();
            uv[t + 1] = vertexData.getFloat();
            if (!Float.isFinite(uv[t]) || !Float.isFinite(uv[t + 1])) {
                throw new IOException("non-finite SKATE sky texture coordinate");
            }
        }
        ByteBuffer indexData = ByteBuffer.wrap(reader.stored(Math.multiplyExact(indexCount, 4)))
                .order(ByteOrder.LITTLE_ENDIAN);
        int[] indices = new int[indexCount];
        for (int i = 0; i < indexCount; i++) indices[i] = checkedIndex(indexData.getInt(), vertexCount);
        if (reader.remaining() != 0) throw new IOException("SKATE sky extension has unexplained trailing bytes");
        int diffuseIndex = textures.size();
        textures.add(new TextureData("retail_sky", width, height, diffuse));
        return new PendingSky(positions, uv, indices, fogNear, fogFar, fogPower, fogMaximum,
                fogColor, sunDirection, sunScale, exposure, diffuseIndex,
                (long) width * height + (long) sunWidth * sunHeight);
    }

    private static void beginUpload(Minecraft client, ParsedModel parsed, long generation) {
        Identifier[] textureIds = new Identifier[parsed.textures.size() + 1];
        textureIds[0] = registerWhite(client, parsed.filename);
        uploadTask = new UploadTask(parsed, textureIds, generation);
    }

    private static void uploadTexture(Minecraft client, UploadTask task, int index) {
        TextureData texture = task.parsed.textures.get(index);
        NativeImage image = new NativeImage(texture.width, texture.height, false);
        boolean transferred = false;
        try {
            image.getPixelBytes().put(texture.rgba);
            Identifier id = Identifier.fromNamespaceAndPath(
                    "pexskate", "map/" + safePath(task.parsed.filename) + "/texture_" + index);
            client.getTextureManager().register(id, new DynamicTexture(
                    () -> "PEX Skate " + task.parsed.mapName + " " + texture.name, image));
            TEXTURES.add(id);
            task.textureIds[index + 1] = id;
            texture.rgba = null;
            transferred = true;
        } finally {
            if (!transferred) image.close();
        }
    }

    private static void sampleIndirectLight(List<TextureData> textures, long textureId, float strength,
                                            float u, float v, float[] output, int offset) {
        if (textureId == 0 || strength <= 0.0f) {
            output[offset] = output[offset + 1] = output[offset + 2] = 1.0f;
            return;
        }
        TextureData texture = textures.get((int) textureId - 1);
        float xPosition = Math.clamp(u, 0.0f, 1.0f) * (texture.width - 1);
        float yPosition = Math.clamp(v, 0.0f, 1.0f) * (texture.height - 1);
        int x0 = (int) xPosition, y0 = (int) yPosition;
        int x1 = Math.min(x0 + 1, texture.width - 1), y1 = Math.min(y0 + 1, texture.height - 1);
        float xBlend = xPosition - x0, yBlend = yPosition - y0;
        for (int channel = 0; channel < 3; channel++) {
            float topLeft = texturePixel(texture, x0, y0, channel);
            float topRight = texturePixel(texture, x1, y0, channel);
            float bottomLeft = texturePixel(texture, x0, y1, channel);
            float bottomRight = texturePixel(texture, x1, y1, channel);
            float top = topLeft + (topRight - topLeft) * xBlend;
            float bottom = bottomLeft + (bottomRight - bottomLeft) * xBlend;
            float baked = top + (bottom - top) * yBlend;
            output[offset + channel] = 1.0f + strength * (baked - 1.0f);
        }
    }

    private static float texturePixel(TextureData texture, int x, int y, int channel) {
        return (texture.rgba[(y * texture.width + x) * 4 + channel] & 255) / 255.0f;
    }

    private static float[] primitiveBounds(float[] positions, int[] indices) {
        float[] bounds = {Float.POSITIVE_INFINITY, Float.POSITIVE_INFINITY, Float.POSITIVE_INFINITY,
                Float.NEGATIVE_INFINITY, Float.NEGATIVE_INFINITY, Float.NEGATIVE_INFINITY};
        for (int index : indices) {
            int point = index * 3;
            for (int axis = 0; axis < 3; axis++) {
                bounds[axis] = Math.min(bounds[axis], positions[point + axis]);
                bounds[axis + 3] = Math.max(bounds[axis + 3], positions[point + axis]);
            }
        }
        return bounds;
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

    private record Model(String filename, String mapSha256, String propsSha256, List<Primitive> primitives, Sky sky) {}
    public record FogSettings(float near, float far, float power, float maximum,
                              float red, float green, float blue) {}
    private record ParsedModel(String filename, String mapName, String mapSha256, String propsSha256,
                               List<TextureData> textures,
                               List<PendingPrimitive> primitives, PendingSky sky, long memoryEstimate) {}
    private record PendingSky(float[] positions, float[] uv, int[] indices,
                              float fogNear, float fogFar, float fogPower, float fogMaximum,
                              float[] fogColor, float[] sunDirection, float sunScale, float exposure,
                              int diffuseTextureIndex, long texturePixels) {}
    private static final class TextureData {
        private final String name;
        private final int width, height;
        private byte[] rgba;

        private TextureData(String name, int width, int height, byte[] rgba) {
            this.name = name;
            this.width = width;
            this.height = height;
            this.rgba = rgba;
        }
    }
    private record PendingPrimitive(float[] positions, float[] normals, float[] uv, float[] vertexIndirectLight,
                                    int[] indices, float[] bounds,
                                    int textureIndex, float[] color, int alphaMode) {}
    private record ChunkMaterial(int material, int x, int z) {}

    private static final class Sky {
        private final float[] positions, uv;
        private final int[] indices;
        private final Identifier diffuseTexture;
        @SuppressWarnings("unused")
        private final float fogNear, fogFar, fogPower, fogMaximum, sunScale, exposure;
        @SuppressWarnings("unused")
        private final float[] fogColor, sunDirection;

        private Sky(PendingSky data, Identifier diffuseTexture) {
            positions = data.positions;
            uv = data.uv;
            indices = data.indices;
            this.diffuseTexture = diffuseTexture;
            fogNear = data.fogNear;
            fogFar = data.fogFar;
            fogPower = data.fogPower;
            fogMaximum = data.fogMaximum;
            fogColor = data.fogColor;
            sunDirection = data.sunDirection;
            sunScale = data.sunScale;
            exposure = data.exposure;
        }

        private void draw(PoseStack.Pose pose, VertexConsumer output, Vec3 camera) {
            Vector3f normal = new Vector3f(0.0f, 1.0f, 0.0f);
            for (int index : indices) {
                int point = index * 3;
                int texCoord = index * 2;
                output.addVertex(pose,
                                (float) camera.x + positions[point],
                                (float) camera.y + positions[point + 1],
                                (float) camera.z + positions[point + 2])
                        .setColor(1.0f, 1.0f, 1.0f, 1.0f)
                        .setUv(uv[texCoord], uv[texCoord + 1])
                        .setLight(0x00F000F0)
                        .setNormal(normal.x, normal.y, normal.z);
            }
        }

        private FogSettings fogSettings() {
            return new FogSettings(fogNear, fogFar, fogPower, fogMaximum,
                    fogColor[0], fogColor[1], fogColor[2]);
        }
    }

    private static final class UploadTask {
        private final ParsedModel parsed;
        private final Identifier[] textureIds;
        private final long generation;
        private int nextTexture;

        private UploadTask(ParsedModel parsed, Identifier[] textureIds, long generation) {
            this.parsed = parsed;
            this.textureIds = textureIds;
            this.generation = generation;
        }
    }

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
        private final float[] positions, normals, uv, vertexIndirectLight, color;
        private final int[] indices;
        private final Identifier texture;
        private final float[] bounds;
        private final int alphaMode;

        private Primitive(float[] positions, float[] normals, float[] uv, float[] vertexIndirectLight, int[] indices,
                          float[] bounds, Identifier texture, float[] color, int alphaMode) {
            this.positions = positions;
            this.normals = normals;
            this.uv = uv;
            this.vertexIndirectLight = vertexIndirectLight;
            this.indices = indices;
            this.texture = texture;
            this.color = color.clone();
            this.bounds = bounds;
            this.alphaMode = alphaMode;
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

        private double centerDistanceSquared(Vec3 point) {
            double dx = (bounds[0] + bounds[3]) * 0.5 - point.x;
            double dy = (bounds[1] + bounds[4]) * 0.5 - point.y;
            double dz = (bounds[2] + bounds[5]) * 0.5 - point.z;
            return dx * dx + dy * dy + dz * dz;
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
                        .setColor(color[0] * vertexIndirectLight[point],
                                color[1] * vertexIndirectLight[point + 1],
                                color[2] * vertexIndirectLight[point + 2], color[3])
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

        private void skipStored(int expected) throws IOException {
            long method = u32();
            int length = checkedInt(u32(), data.remaining(), "stored block length");
            if (method > 1 || (method == 0 && length != expected)) {
                throw new IOException("invalid SKATE extension storage block");
            }
            skip(length);
        }

        private int remaining() { return data.remaining(); }
    }
}
