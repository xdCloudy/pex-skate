package dev.pexskate.client;

import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;

public record SkateStatePayload(byte version, boolean active, int entityId, double x, double y, double z, float yaw, float roll)
        implements CustomPacketPayload {
    public static final Type<SkateStatePayload> TYPE = new Type<>(Identifier.fromNamespaceAndPath("pexskate", "state"));
    public static final StreamCodec<RegistryFriendlyByteBuf, SkateStatePayload> CODEC = StreamCodec.of(
            (buffer, payload) -> {
                buffer.writeByte(payload.version());
                buffer.writeBoolean(payload.active());
                buffer.writeInt(payload.entityId());
                buffer.writeDouble(payload.x());
                buffer.writeDouble(payload.y());
                buffer.writeDouble(payload.z());
                buffer.writeFloat(payload.yaw());
                buffer.writeFloat(payload.roll());
            },
            buffer -> new SkateStatePayload(
                    buffer.readByte(),
                    buffer.readBoolean(),
                    buffer.readInt(),
                    buffer.readDouble(),
                    buffer.readDouble(),
                    buffer.readDouble(),
                    buffer.readFloat(),
                    buffer.readFloat()
            )
    );

    @Override
    public Type<? extends CustomPacketPayload> type() {
        return TYPE;
    }
}
