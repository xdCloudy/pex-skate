package dev.pexskate.client;

import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;
import java.util.List;

public record SkateStatePayload(
        byte version,
        boolean active,
        int entityId,
        float x, float y, float z,
        float boardPosX, float boardPosY, float boardPosZ,
        float boardRotX, float boardRotY, float boardRotZ, float boardRotW,
        float velocityX, float velocityY, float velocityZ,
        float sequenceScore, float lineScore, float totalScore, float multiplier, float lineTime,
        byte scoreFlags,
        String trickName,
        float hipsX, float hipsY, float hipsZ,
        long boneMask,
        short[] boneOffsets,
        short[] boneRotations,
        float cameraOffsetX, float cameraOffsetY, float cameraOffsetZ,
        float cameraForwardX, float cameraForwardY, float cameraForwardZ,
        float cameraUpX, float cameraUpY, float cameraUpZ,
        float cameraFov,
        boolean mapBacked,
        String mapFilename,
        String mapSha256,
        String propsSha256,
        int acknowledgedControlSequence,
        List<PropTransform> propTransforms
) implements CustomPacketPayload {
    public record PropTransform(long identity, float x, float y, float z,
                                float yaw, float pivotX, float pivotY, float pivotZ) {}

    public static final Type<SkateStatePayload> TYPE = new Type<>(Identifier.fromNamespaceAndPath("pexskate", "state"));
    public static final StreamCodec<RegistryFriendlyByteBuf, SkateStatePayload> CODEC = StreamCodec.of(
            (buffer, payload) -> {
                buffer.writeByte(payload.version());
                buffer.writeBoolean(payload.active());
                buffer.writeInt(payload.entityId());
                for (float value : new float[] {
                        payload.x(), payload.y(), payload.z(),
                        payload.boardPosX(), payload.boardPosY(), payload.boardPosZ(),
                        payload.boardRotX(), payload.boardRotY(), payload.boardRotZ(), payload.boardRotW(),
                        payload.velocityX(), payload.velocityY(), payload.velocityZ(),
                        payload.sequenceScore(), payload.lineScore(), payload.totalScore(),
                        payload.multiplier(), payload.lineTime()
                }) buffer.writeFloat(value);
                buffer.writeByte(payload.scoreFlags());
                byte[] trick = payload.trickName().getBytes(java.nio.charset.StandardCharsets.UTF_8);
                int length = Math.min(trick.length, 192);
                buffer.writeShort(length);
                buffer.writeBytes(trick, 0, length);
                buffer.writeFloat(payload.hipsX());
                buffer.writeFloat(payload.hipsY());
                buffer.writeFloat(payload.hipsZ());
                buffer.writeLong(payload.boneMask());
                for (short offset : payload.boneOffsets()) buffer.writeShort(offset);
                for (float value : new float[] {
                        payload.cameraOffsetX(), payload.cameraOffsetY(), payload.cameraOffsetZ(),
                        payload.cameraForwardX(), payload.cameraForwardY(), payload.cameraForwardZ(),
                        payload.cameraUpX(), payload.cameraUpY(), payload.cameraUpZ(), payload.cameraFov()
                }) buffer.writeFloat(value);
                if (payload.version() >= 5) {
                    if (payload.boneRotations().length != 36 * 4) {
                        throw new IllegalArgumentException("PEX Skate state must contain 36 joint rotations");
                    }
                    for (short rotation : payload.boneRotations()) buffer.writeShort(rotation);
                }
                if (payload.version() >= 6) buffer.writeBoolean(payload.mapBacked());
                if (payload.version() >= 7) {
                    byte[] mapFilename = payload.mapFilename().getBytes(java.nio.charset.StandardCharsets.UTF_8);
                    if (mapFilename.length > 512) throw new IllegalArgumentException("PEX Skate map filename exceeds protocol limit");
                    buffer.writeShort(mapFilename.length);
                    buffer.writeBytes(mapFilename);
                }
                if (payload.version() >= 8) {
                    writeDigest(buffer, payload.mapSha256());
                    writeDigest(buffer, payload.propsSha256());
                }
                if (payload.version() >= 9) buffer.writeInt(payload.acknowledgedControlSequence());
                if (payload.version() >= 10) {
                    if (payload.propTransforms().size() > 8192) {
                        throw new IllegalArgumentException("PEX Skate prop update count exceeds protocol limit");
                    }
                    buffer.writeShort(payload.propTransforms().size());
                    for (PropTransform transform : payload.propTransforms()) {
                        buffer.writeLong(transform.identity());
                        buffer.writeFloat(transform.x());
                        buffer.writeFloat(transform.y());
                        buffer.writeFloat(transform.z());
                        if (payload.version() >= 11) {
                            buffer.writeFloat(transform.yaw());
                            buffer.writeFloat(transform.pivotX());
                            buffer.writeFloat(transform.pivotY());
                            buffer.writeFloat(transform.pivotZ());
                        }
                    }
                }
            },
            buffer -> {
                byte version = buffer.readByte();
                if (version != 4 && version != 5 && version != 6 && version != 7 && version != 8
                        && version != 9 && version != 10 && version != 11) {
                    throw new IllegalArgumentException("Unsupported PEX Skate state version " + version);
                }
                boolean active = buffer.readBoolean();
                int entityId = buffer.readInt();
                float[] values = new float[18];
                for (int i = 0; i < values.length; i++) values[i] = buffer.readFloat();
                byte flags = buffer.readByte();
                int length = buffer.readUnsignedShort();
                if (length > 192) throw new IllegalArgumentException("PEX Skate trick name exceeds protocol limit");
                byte[] trick = new byte[length];
                buffer.readBytes(trick);
                float hipsX = buffer.readFloat();
                float hipsY = buffer.readFloat();
                float hipsZ = buffer.readFloat();
                long boneMask = buffer.readLong();
                short[] offsets = new short[36 * 3];
                for (int i = 0; i < offsets.length; i++) offsets[i] = buffer.readShort();
                float[] camera = new float[10];
                for (int i = 0; i < camera.length; i++) camera[i] = buffer.readFloat();
                short[] rotations = identityRotations();
                if (version >= 5) {
                    for (int i = 0; i < rotations.length; i++) rotations[i] = buffer.readShort();
                }
                boolean mapBacked = version >= 6 && buffer.readBoolean();
                String mapFilename = "";
                if (version >= 7) {
                    int mapFilenameLength = buffer.readUnsignedShort();
                    if (mapFilenameLength > 512) throw new IllegalArgumentException("PEX Skate map filename exceeds protocol limit");
                    byte[] mapFilenameBytes = new byte[mapFilenameLength];
                    buffer.readBytes(mapFilenameBytes);
                    mapFilename = new String(mapFilenameBytes, java.nio.charset.StandardCharsets.UTF_8);
                }
                String mapSha256 = version >= 8 ? readDigest(buffer) : "";
                String propsSha256 = version >= 8 ? readDigest(buffer) : "";
                int acknowledgedControlSequence = version >= 9 ? buffer.readInt() : 0;
                List<PropTransform> propTransforms = List.of();
                if (version >= 10) {
                    int propCount = buffer.readUnsignedShort();
                    if (propCount > 8192) throw new IllegalArgumentException("PEX Skate prop update count exceeds protocol limit");
                    var updates = new java.util.ArrayList<PropTransform>(propCount);
                    for (int i = 0; i < propCount; i++) {
                        long identity = buffer.readLong();
                        float x = buffer.readFloat();
                        float y = buffer.readFloat();
                        float z = buffer.readFloat();
                        float yaw = version >= 11 ? buffer.readFloat() : 0.0f;
                        float pivotX = version >= 11 ? buffer.readFloat() : 0.0f;
                        float pivotY = version >= 11 ? buffer.readFloat() : 0.0f;
                        float pivotZ = version >= 11 ? buffer.readFloat() : 0.0f;
                        updates.add(new PropTransform(identity, x, y, z, yaw, pivotX, pivotY, pivotZ));
                    }
                    propTransforms = List.copyOf(updates);
                }
                return new SkateStatePayload(version, active, entityId,
                        values[0], values[1], values[2], values[3], values[4], values[5],
                        values[6], values[7], values[8], values[9],
                        values[10], values[11], values[12], values[13], values[14],
                        values[15], values[16], values[17],
                        flags, new String(trick, java.nio.charset.StandardCharsets.UTF_8),
                        hipsX, hipsY, hipsZ, boneMask, offsets, rotations,
                        camera[0], camera[1], camera[2], camera[3], camera[4], camera[5],
                        camera[6], camera[7], camera[8], camera[9], mapBacked, mapFilename,
                        mapSha256, propsSha256, acknowledgedControlSequence, propTransforms);
            }
    );

    private static short[] identityRotations() {
        short[] rotations = new short[36 * 4];
        for (int i = 0; i < 36; i++) rotations[i * 4 + 3] = Short.MAX_VALUE;
        return rotations;
    }

    private static void writeDigest(RegistryFriendlyByteBuf buffer, String digest) {
        byte[] bytes = digest == null || digest.isBlank()
                ? new byte[32] : java.util.HexFormat.of().parseHex(digest);
        if (bytes.length != 32) throw new IllegalArgumentException("PEX Skate digest must be SHA-256");
        buffer.writeBytes(bytes);
    }

    private static String readDigest(RegistryFriendlyByteBuf buffer) {
        byte[] bytes = new byte[32];
        buffer.readBytes(bytes);
        boolean empty = true;
        for (byte value : bytes) empty &= value == 0;
        return empty ? "" : java.util.HexFormat.of().formatHex(bytes);
    }

    @Override
    public Type<? extends CustomPacketPayload> type() {
        return TYPE;
    }
}
