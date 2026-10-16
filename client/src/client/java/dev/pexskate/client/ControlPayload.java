package dev.pexskate.client;

import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;

public record ControlPayload(byte version, byte flags, byte leftTrigger, byte rightTrigger,
                             byte forward, byte steer, byte rightX, byte rightY, byte modifiers,
                             int sequence)
        implements CustomPacketPayload {
    public static final Type<ControlPayload> TYPE = new Type<>(Identifier.fromNamespaceAndPath("pexskate", "control"));
    public static final StreamCodec<RegistryFriendlyByteBuf, ControlPayload> CODEC = StreamCodec.of(
            (buffer, payload) -> {
                buffer.writeByte(payload.version());
                buffer.writeByte(payload.flags());
                buffer.writeByte(payload.leftTrigger());
                buffer.writeByte(payload.rightTrigger());
                buffer.writeByte(payload.forward());
                buffer.writeByte(payload.steer());
                buffer.writeByte(payload.rightX());
                buffer.writeByte(payload.rightY());
                buffer.writeByte(payload.modifiers());
                if (payload.version() >= 9) buffer.writeInt(payload.sequence());
            },
            buffer -> {
                byte version = buffer.readByte();
                byte flags = buffer.readByte();
                byte leftTrigger = buffer.readByte();
                byte rightTrigger = buffer.readByte();
                byte forward = buffer.readByte();
                byte steer = buffer.readByte();
                byte rightX = buffer.readByte();
                byte rightY = buffer.readByte();
                byte modifiers = buffer.readByte();
                int sequence = version >= 9 ? buffer.readInt() : 0;
                return new ControlPayload(version, flags, leftTrigger, rightTrigger,
                        forward, steer, rightX, rightY, modifiers, sequence);
            }
    );

    @Override
    public Type<? extends CustomPacketPayload> type() {
        return TYPE;
    }
}
