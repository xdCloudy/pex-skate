package dev.pexskate.client;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.blaze3d.platform.NativeImage;
import com.mojang.blaze3d.vertex.PoseStack;
import com.mojang.blaze3d.vertex.VertexConsumer;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.SubmitNodeCollector;
import net.minecraft.client.renderer.rendertype.RenderTypes;
import net.minecraft.client.renderer.rendertype.RenderType;
import net.minecraft.client.renderer.texture.DynamicTexture;
import net.minecraft.resources.Identifier;
import net.minecraft.world.phys.Vec3;
import org.joml.Matrix3f;
import org.joml.Matrix4f;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/** Loads the user's converted skinned GLB and skins it from authoritative Skate joints. */
public final class SkateSkaterModel {
    private static final Logger LOGGER = LoggerFactory.getLogger("pexskate/skater-model");
    private static final int MAX_GLB_BYTES = 128 * 1024 * 1024;
    private static final String[] BONE_NAMES = {
            "TRAJECTORY", "HIPS", "SPINE", "SPINE1", "SPINE2", "SPINE3", "NECK", "NECK1", "HEAD",
            "RIGHTSHOULDER", "RIGHTARM", "RIGHTFOREARM", "RIGHTHAND", "LEFTSHOULDER", "LEFTARM",
            "LEFTFOREARM", "LEFTHAND", "RIGHTUPLEG", "RIGHTLEG", "RIGHTFOOT", "RIGHTTOEBASE",
            "LEFTUPLEG", "LEFTLEG", "LEFTFOOT", "LEFTTOEBASE", "SKATEBOARD_ROOT", "TRUCK_FRONT",
            "RIGHT_WHEELFRONT", "LEFT_WHEELFRONT", "TRUCK_BACK", "LEFT_WHEELBACK", "RIGHT_WHEELBACK",
            "RIGHTTOEBASE_REPARENTED", "LEFTTOEBASE_REPARENTED", "RIGHTHAND_REPARENTED", "LEFTHAND_REPARENTED"
    };

    private static final Map<String, Integer> BONE_SLOTS = new HashMap<>();
    private static final List<Identifier> TEXTURES = new ArrayList<>();
    private static Model model;

    static {
        for (int i = 0; i < BONE_NAMES.length; i++) BONE_SLOTS.put(BONE_NAMES[i], i);
    }

    private SkateSkaterModel() {}

    static void load() {
        close();
        Path path = Minecraft.getInstance().gameDirectory.toPath()
                .resolve("config/pex-skate/private/skater.glb");
        if (!Files.isRegularFile(path)) {
            LOGGER.info("No converted skater model at {}; using the Minecraft player rig", path);
            return;
        }
        try {
            if (Files.size(path) > MAX_GLB_BYTES) throw new IOException("GLB exceeds 128 MiB limit");
            model = parse(Files.readAllBytes(path));
            LOGGER.info("Loaded user skater model from {}", path);
        } catch (Exception error) {
            close();
            LOGGER.error("Could not load converted Skate 3 skater model from {}", path, error);
        }
    }

    public static boolean isReady() { return model != null; }

    static void close() {
        Minecraft client = Minecraft.getInstance();
        for (Identifier texture : TEXTURES) client.getTextureManager().release(texture);
        TEXTURES.clear();
        model = null;
    }

    static void submit(PoseStack matrices, SubmitNodeCollector collector, int light, int overlay,
                       SkatePoseCache.Pose pose) {
        Model current = model;
        if (current == null || pose == null) return;
        Vec3 camera = Minecraft.getInstance().gameRenderer.mainCamera().position();
        Matrix4f inversePose = new Matrix4f(matrices.last().pose()).invert();
        Matrix3f inverseNormal = new Matrix3f(matrices.last().normal()).invert();
        // Every GLB primitive uses this same 36-joint snapshot. Decode and
        // interpolate it once per skater submission instead of allocating
        // point/quaternion arrays again for every material primitive.
        float[][] jointPoints = new float[BONE_NAMES.length][];
        float[][] jointRotations = new float[BONE_NAMES.length][];
        for (int slot = 0; slot < BONE_NAMES.length; slot++) {
            if (pose.has(slot)) {
                jointPoints[slot] = pose.point(slot);
                jointRotations[slot] = pose.rotation(slot);
            }
        }
        for (Primitive primitive : current.primitives) {
            RenderType renderType = primitive.translucent
                    ? RenderTypes.entityTranslucent(primitive.texture)
                    : RenderTypes.entityCutoutCull(primitive.texture);
            collector.submitCustomGeometry(matrices, renderType, (stackPose, vertices) ->
                    primitive.draw(stackPose, vertices, jointPoints, jointRotations,
                            camera, inversePose, inverseNormal, light, overlay));
        }
    }

    private static Model parse(byte[] file) throws IOException {
        if (file.length < 20) throw new IOException("GLB header is truncated");
        ByteBuffer header = ByteBuffer.wrap(file).order(ByteOrder.LITTLE_ENDIAN);
        if (header.getInt() != 0x46546c67 || header.getInt() != 2 || header.getInt() != file.length) {
            throw new IOException("Invalid GLB header");
        }
        byte[] json = null;
        byte[] binary = null;
        while (header.remaining() >= 8) {
            int length = header.getInt();
            int type = header.getInt();
            if (length < 0 || length > header.remaining()) throw new IOException("Invalid GLB chunk length");
            byte[] chunk = new byte[length];
            header.get(chunk);
            if (type == 0x4e4f534a && json == null) json = chunk;
            else if (type == 0x004e4942 && binary == null) binary = chunk;
        }
        if (json == null || binary == null) throw new IOException("GLB must contain JSON and binary chunks");
        JsonObject document = JsonParser.parseString(new String(json, StandardCharsets.UTF_8).strip()).getAsJsonObject();
        JsonArray views = array(document, "bufferViews");
        ByteBuffer data = ByteBuffer.wrap(binary).order(ByteOrder.LITTLE_ENDIAN);
        for (JsonElement buffer : array(document, "buffers")) {
            JsonObject item = buffer.getAsJsonObject();
            if (item.has("uri")) throw new IOException("External GLB buffers are unsupported");
            if (item.get("byteLength").getAsInt() > binary.length) throw new IOException("GLB buffer is truncated");
        }

        JsonArray nodes = array(document, "nodes");
        JsonArray skins = array(document, "skins");
        if (skins.isEmpty()) throw new IOException("GLB has no skinned character");
        JsonObject skin = skins.get(0).getAsJsonObject();
        JsonArray joints = array(skin, "joints");
        int[] jointSlots = new int[joints.size()];
        for (int i = 0; i < joints.size(); i++) {
            int nodeIndex = joints.get(i).getAsInt();
            String name = nodes.get(nodeIndex).getAsJsonObject().get("name").getAsString();
            Integer slot = BONE_SLOTS.get(name);
            if (slot == null) throw new IOException("GLB joint is absent from the Skate rig: " + name);
            jointSlots[i] = slot;
        }
        int inverseAccessor = skin.get("inverseBindMatrices").getAsInt();
        float[] inverseBinds = floats(document, views, data, inverseAccessor, 16);
        if (inverseBinds.length != joints.size() * 16) throw new IOException("GLB inverse bind matrix count does not match its joints");

        JsonArray imageDefinitions = array(document, "images");
        List<Identifier> imageIds = new ArrayList<>();
        Minecraft client = Minecraft.getInstance();
        for (int i = 0; i < imageDefinitions.size(); i++) {
            int imageNumber = i;
            JsonObject image = imageDefinitions.get(i).getAsJsonObject();
            if (image.has("uri")) throw new IOException("External GLB images are unsupported");
            int viewIndex = image.get("bufferView").getAsInt();
            ByteBuffer encoded = viewData(views, data, viewIndex);
            byte[] bytes = new byte[encoded.remaining()];
            encoded.get(bytes);
            NativeImage pixels = NativeImage.read(bytes);
            Identifier id = Identifier.fromNamespaceAndPath("pexskate", "skater/image_" + i);
            client.getTextureManager().register(id, new DynamicTexture(() -> "PEX Skate skater image " + imageNumber, pixels));
            TEXTURES.add(id);
            imageIds.add(id);
        }

        JsonArray gltfTextures = array(document, "textures");
        JsonArray materials = array(document, "materials");
        JsonArray gltfMeshes = array(document, "meshes");
        if (gltfMeshes.isEmpty()) throw new IOException("GLB has no mesh");
        List<Primitive> primitives = new ArrayList<>();
        long totalVertices = 0;
        long totalIndices = 0;
        for (JsonElement primitiveElement : array(gltfMeshes.get(0).getAsJsonObject(), "primitives")) {
            JsonObject primitive = primitiveElement.getAsJsonObject();
            if (primitive.has("mode") && primitive.get("mode").getAsInt() != 4) {
                throw new IOException("Only triangle GLB primitives are supported");
            }
            JsonObject attributes = primitive.getAsJsonObject("attributes");
            float[] positions = floats(document, views, data, attributes.get("POSITION").getAsInt(), 3);
            float[] normals = floats(document, views, data, attributes.get("NORMAL").getAsInt(), 3);
            float[] uv = floats(document, views, data, attributes.get("TEXCOORD_0").getAsInt(), 2);
            float[] weights = floats(document, views, data, attributes.get("WEIGHTS_0").getAsInt(), 4);
            int[] vertexJoints = integers(document, views, data, attributes.get("JOINTS_0").getAsInt());
            int[] indices = integers(document, views, data, primitive.get("indices").getAsInt());
            totalVertices += positions.length / 3;
            totalIndices += indices.length;
            if (totalVertices > 2_000_000 || totalIndices > 6_000_000) {
                throw new IOException("GLB character exceeds the total client mesh size limit");
            }
            if (positions.length / 3 > 2_000_000 || indices.length > 6_000_000) {
                throw new IOException("GLB primitive exceeds the client mesh size limit");
            }
            if (positions.length % 3 != 0 || normals.length != positions.length || uv.length / 2 != positions.length / 3
                    || weights.length / 4 != positions.length / 3 || vertexJoints.length != weights.length
                    || indices.length % 3 != 0) throw new IOException("GLB mesh attribute counts do not match");
            if (!finite(positions) || !finite(normals) || !finite(uv) || !finite(weights) || !finite(inverseBinds)) {
                throw new IOException("GLB contains non-finite mesh or rig values");
            }
            for (int i = 0; i < weights.length; i += 4) {
                float total = weights[i] + weights[i + 1] + weights[i + 2] + weights[i + 3];
                if (weights[i] < 0.0f || weights[i + 1] < 0.0f || weights[i + 2] < 0.0f || weights[i + 3] < 0.0f
                        || total < 0.99f || total > 1.01f) throw new IOException("GLB has invalid skin weights");
            }
            for (int index : indices) if (index < 0 || index >= positions.length / 3) throw new IOException("GLB index is outside the vertex array");
            for (int joint : vertexJoints) if (joint < 0 || joint >= jointSlots.length) throw new IOException("GLB skin index is outside the joint array");

            int materialIndex = primitive.has("material") ? primitive.get("material").getAsInt() : -1;
            Identifier texture = null;
            float[] tint = {1.0f, 1.0f, 1.0f, 1.0f};
            boolean translucent = false;
            if (materialIndex >= 0) {
                JsonObject material = materials.get(materialIndex).getAsJsonObject();
                translucent = material.has("alphaMode") && material.get("alphaMode").getAsString().equals("BLEND");
                JsonObject pbr = material.getAsJsonObject("pbrMetallicRoughness");
                if (pbr != null) {
                    if (pbr.has("baseColorFactor")) tint = floats(pbr.getAsJsonArray("baseColorFactor"));
                    if (pbr.has("baseColorTexture")) {
                        int textureIndex = pbr.getAsJsonObject("baseColorTexture").get("index").getAsInt();
                        int imageIndex = gltfTextures.get(textureIndex).getAsJsonObject().get("source").getAsInt();
                        texture = imageIds.get(imageIndex);
                    }
                }
            }
            if (!finite(tint) || tint.length != 4) throw new IOException("GLB material tint is invalid");
            for (int i = 0; i < tint.length; i++) tint[i] = Math.clamp(tint[i], 0.0f, 1.0f);
            if (texture == null) texture = registerWhiteTexture(client);
            primitives.add(new Primitive(positions, normals, uv, weights, vertexJoints, indices,
                    jointSlots, inverseBinds, texture, tint, translucent));
        }
        if (primitives.isEmpty()) throw new IOException("GLB contains no renderable triangles");
        return new Model(List.copyOf(primitives));
    }

    private static Identifier registerWhiteTexture(Minecraft client) {
        Identifier id = Identifier.fromNamespaceAndPath("pexskate", "skater/white");
        if (!TEXTURES.contains(id)) {
            NativeImage pixels = new NativeImage(1, 1, false);
            pixels.setPixel(0, 0, 0xffffffff);
            client.getTextureManager().register(id, new DynamicTexture(() -> "PEX Skate white", pixels));
            TEXTURES.add(id);
        }
        return id;
    }

    private static JsonArray array(JsonObject object, String key) throws IOException {
        JsonElement element = object.get(key);
        if (element == null || !element.isJsonArray()) throw new IOException("GLB is missing array " + key);
        return element.getAsJsonArray();
    }

    private static ByteBuffer viewData(JsonArray views, ByteBuffer data, int index) throws IOException {
        if (index < 0 || index >= views.size()) throw new IOException("GLB references an invalid buffer view");
        JsonObject view = views.get(index).getAsJsonObject();
        int offset = view.has("byteOffset") ? view.get("byteOffset").getAsInt() : 0;
        int length = view.get("byteLength").getAsInt();
        if (offset < 0 || length < 0 || offset > data.limit() - length) throw new IOException("GLB buffer view is outside the binary chunk");
        ByteBuffer result = data.duplicate().order(ByteOrder.LITTLE_ENDIAN);
        result.position(offset).limit(offset + length);
        return result.slice().order(ByteOrder.LITTLE_ENDIAN);
    }

    private static float[] floats(JsonObject document, JsonArray views, ByteBuffer data, int accessorIndex, int expectedComponents) throws IOException {
        Accessor accessor = accessor(document, views, data, accessorIndex, expectedComponents);
        if (accessor.componentType != 5126) throw new IOException("Expected float GLB accessor");
        float[] result = new float[accessor.count * expectedComponents];
        ByteBuffer source = viewData(views, data, accessor.view);
        for (int i = 0; i < accessor.count; i++) {
            int start = accessor.offset + i * accessor.stride;
            for (int c = 0; c < expectedComponents; c++) result[i * expectedComponents + c] = source.getFloat(start + c * 4);
        }
        return result;
    }

    private static float[] floats(JsonArray array) {
        float[] result = new float[array.size()];
        for (int i = 0; i < result.length; i++) result[i] = array.get(i).getAsFloat();
        return result;
    }

    private static boolean finite(float[] values) {
        for (float value : values) if (!Float.isFinite(value)) return false;
        return true;
    }

    private static int[] integers(JsonObject document, JsonArray views, ByteBuffer data, int accessorIndex) throws IOException {
        JsonArray accessors = array(document, "accessors");
        if (accessorIndex < 0 || accessorIndex >= accessors.size()) throw new IOException("GLB references an invalid accessor");
        JsonObject json = accessors.get(accessorIndex).getAsJsonObject();
        if (!json.get("type").getAsString().equals("SCALAR") && !json.get("type").getAsString().equals("VEC4")) {
            throw new IOException("Expected scalar or VEC4 integer GLB accessor");
        }
        int components = json.get("type").getAsString().equals("VEC4") ? 4 : 1;
        Accessor accessor = accessor(document, views, data, accessorIndex, components);
        if (accessor.componentType != 5121 && accessor.componentType != 5123 && accessor.componentType != 5125) {
            throw new IOException("Expected unsigned integer GLB accessor");
        }
        int size = accessor.componentType == 5121 ? 1 : accessor.componentType == 5123 ? 2 : 4;
        int[] result = new int[accessor.count * components];
        ByteBuffer source = viewData(views, data, accessor.view);
        for (int i = 0; i < accessor.count; i++) {
            int start = accessor.offset + i * accessor.stride;
            for (int c = 0; c < components; c++) {
                int at = start + c * size;
                result[i * components + c] = size == 1 ? Byte.toUnsignedInt(source.get(at))
                        : size == 2 ? Short.toUnsignedInt(source.getShort(at)) : source.getInt(at);
            }
        }
        return result;
    }

    private static Accessor accessor(JsonObject document, JsonArray views, ByteBuffer data, int index, int components) throws IOException {
        JsonArray accessors = array(document, "accessors");
        if (index < 0 || index >= accessors.size()) throw new IOException("GLB references an invalid accessor");
        JsonObject json = accessors.get(index).getAsJsonObject();
        if (json.has("sparse")) throw new IOException("Sparse GLB accessors are unsupported");
        String type = json.get("type").getAsString();
        int actualComponents = switch (type) { case "SCALAR" -> 1; case "VEC2" -> 2; case "VEC3" -> 3; case "VEC4" -> 4; case "MAT4" -> 16; default -> -1; };
        if (actualComponents != components) throw new IOException("Unexpected GLB accessor type " + type);
        int viewIndex = json.get("bufferView").getAsInt();
        JsonObject view = views.get(viewIndex).getAsJsonObject();
        int componentType = json.get("componentType").getAsInt();
        int bytes = componentType == 5121 ? 1 : componentType == 5123 ? 2 : 4;
        int stride = view.has("byteStride") ? view.get("byteStride").getAsInt() : components * bytes;
        int offset = json.has("byteOffset") ? json.get("byteOffset").getAsInt() : 0;
        int count = json.get("count").getAsInt();
        int maximumCount = components == 1 ? 6_000_000 : components == 16 ? 4096 : 2_000_000;
        if (count < 0 || count > maximumCount) throw new IOException("GLB accessor exceeds the count limit");
        ByteBuffer section = viewData(views, data, viewIndex);
        if (offset < 0 || stride < components * bytes || (long) offset + (long) Math.max(count - 1, 0) * stride + components * bytes > section.limit()) {
            throw new IOException("GLB accessor exceeds its buffer view");
        }
        return new Accessor(viewIndex, offset, stride, count, componentType);
    }

    private record Model(List<Primitive> primitives) {}
    private record Accessor(int view, int offset, int stride, int count, int componentType) {}

    private static final class Primitive {
        private final float[] positions, normals, uv, weights, inverseBinds, tint;
        private final int[] vertexJoints, indices, jointSlots;
        private final Identifier texture;
        private final boolean translucent;
        private final float[] skinnedPositions;
        private final float[] skinnedNormals;

        private Primitive(float[] positions, float[] normals, float[] uv, float[] weights, int[] vertexJoints,
                          int[] indices, int[] jointSlots, float[] inverseBinds, Identifier texture,
                          float[] tint, boolean translucent) {
            this.positions = positions; this.normals = normals; this.uv = uv; this.weights = weights;
            this.vertexJoints = vertexJoints; this.indices = indices; this.jointSlots = jointSlots;
            this.inverseBinds = inverseBinds; this.texture = texture; this.tint = tint;
            this.translucent = translucent;
            this.skinnedPositions = new float[positions.length];
            this.skinnedNormals = new float[normals.length];
        }

        private void draw(PoseStack.Pose matrix, VertexConsumer output,
                          float[][] jointPoints, float[][] jointRotations, Vec3 camera,
                          Matrix4f inversePose, Matrix3f inverseNormal, int light, int overlay) {
            float[] rotatedPosition = new float[3];
            float[] rotatedNormal = new float[3];
            for (int vertex = 0; vertex < positions.length / 3; vertex++) {
                float outX = 0.0f, outY = 0.0f, outZ = 0.0f;
                float normalX = 0.0f, normalY = 0.0f, normalZ = 0.0f;
                float totalWeight = 0.0f;
                for (int influence = 0; influence < 4; influence++) {
                    int at = vertex * 4 + influence;
                    float weight = weights[at];
                    if (!(weight > 0.0f)) continue;
                    int joint = vertexJoints[at];
                    int slot = jointSlots[joint];
                    float[] point = jointPoints[slot];
                    float[] rotation = jointRotations[slot];
                    if (point == null || rotation == null) continue;
                    int p = vertex * 3;
                    float px = positions[p], py = positions[p + 1], pz = positions[p + 2];
                    float nx = normals[p], ny = normals[p + 1], nz = normals[p + 2];
                    int bind = joint * 16;
                    float bindX = inverseBinds[bind] * px + inverseBinds[bind + 4] * py + inverseBinds[bind + 8] * pz + inverseBinds[bind + 12];
                    float bindY = inverseBinds[bind + 1] * px + inverseBinds[bind + 5] * py + inverseBinds[bind + 9] * pz + inverseBinds[bind + 13];
                    float bindZ = inverseBinds[bind + 2] * px + inverseBinds[bind + 6] * py + inverseBinds[bind + 10] * pz + inverseBinds[bind + 14];
                    float bindNx = inverseBinds[bind] * nx + inverseBinds[bind + 4] * ny + inverseBinds[bind + 8] * nz;
                    float bindNy = inverseBinds[bind + 1] * nx + inverseBinds[bind + 5] * ny + inverseBinds[bind + 9] * nz;
                    float bindNz = inverseBinds[bind + 2] * nx + inverseBinds[bind + 6] * ny + inverseBinds[bind + 10] * nz;
                    rotate(rotation, bindX, bindY, bindZ, rotatedPosition);
                    rotate(rotation, bindNx, bindNy, bindNz, rotatedNormal);
                    outX += weight * (rotatedPosition[0] + point[0]);
                    outY += weight * (rotatedPosition[1] + point[1]);
                    outZ += weight * (rotatedPosition[2] + point[2]);
                    normalX += weight * rotatedNormal[0];
                    normalY += weight * rotatedNormal[1];
                    normalZ += weight * rotatedNormal[2];
                    totalWeight += weight;
                }
                if (totalWeight <= 1.0e-6f) {
                    int p = vertex * 3;
                    outX = positions[p]; outY = positions[p + 1]; outZ = positions[p + 2];
                    normalX = normals[p]; normalY = normals[p + 1]; normalZ = normals[p + 2];
                } else if (Math.abs(totalWeight - 1.0f) > 1.0e-5f) {
                    outX /= totalWeight; outY /= totalWeight; outZ /= totalWeight;
                    normalX /= totalWeight; normalY /= totalWeight; normalZ /= totalWeight;
                }
                outX -= (float) camera.x; outY -= (float) camera.y; outZ -= (float) camera.z;
                float localX = inversePose.m00() * outX + inversePose.m10() * outY + inversePose.m20() * outZ + inversePose.m30();
                float localY = inversePose.m01() * outX + inversePose.m11() * outY + inversePose.m21() * outZ + inversePose.m31();
                float localZ = inversePose.m02() * outX + inversePose.m12() * outY + inversePose.m22() * outZ + inversePose.m32();
                float localNormalX = inverseNormal.m00() * normalX + inverseNormal.m10() * normalY + inverseNormal.m20() * normalZ;
                float localNormalY = inverseNormal.m01() * normalX + inverseNormal.m11() * normalY + inverseNormal.m21() * normalZ;
                float localNormalZ = inverseNormal.m02() * normalX + inverseNormal.m12() * normalY + inverseNormal.m22() * normalZ;
                float normalLength = (float) Math.sqrt(localNormalX * localNormalX + localNormalY * localNormalY + localNormalZ * localNormalZ);
                if (normalLength > 1.0e-5f) {
                    localNormalX /= normalLength; localNormalY /= normalLength; localNormalZ /= normalLength;
                }
                int p = vertex * 3;
                skinnedPositions[p] = localX; skinnedPositions[p + 1] = localY; skinnedPositions[p + 2] = localZ;
                skinnedNormals[p] = localNormalX; skinnedNormals[p + 1] = localNormalY; skinnedNormals[p + 2] = localNormalZ;
            }
            for (int index : indices) {
                int p = index * 3;
                output.addVertex(matrix, skinnedPositions[p], skinnedPositions[p + 1], skinnedPositions[p + 2])
                        .setColor(tint[0], tint[1], tint[2], tint[3])
                        .setUv(uv[index * 2], 1.0f - uv[index * 2 + 1])
                        .setOverlay(overlay).setLight(light)
                        .setNormal(matrix, skinnedNormals[p], skinnedNormals[p + 1], skinnedNormals[p + 2]);
            }
        }

        private static void rotate(float[] q, float x, float y, float z, float[] result) {
            float tx = 2.0f * (q[1] * z - q[2] * y);
            float ty = 2.0f * (q[2] * x - q[0] * z);
            float tz = 2.0f * (q[0] * y - q[1] * x);
            result[0] = x + q[3] * tx + q[1] * tz - q[2] * ty;
            result[1] = y + q[3] * ty + q[2] * tx - q[0] * tz;
            result[2] = z + q[3] * tz + q[0] * ty - q[1] * tx;
        }
    }
}
