package dev.pexskate.client;

import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;

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
        short[] boneOffsets
) implements CustomPacketPayload {
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
            },
            buffer -> {
                byte version = buffer.readByte();
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
                return new SkateStatePayload(version, active, entityId,
                        values[0], values[1], values[2], values[3], values[4], values[5],
                        values[6], values[7], values[8], values[9],
                        values[10], values[11], values[12], values[13], values[14],
                        values[15], values[16], values[17],
                        flags, new String(trick, java.nio.charset.StandardCharsets.UTF_8),
                        hipsX, hipsY, hipsZ, boneMask, offsets);
            }
    );

    @Override
    public Type<? extends CustomPacketPayload> type() {
        return TYPE;
    }
}
