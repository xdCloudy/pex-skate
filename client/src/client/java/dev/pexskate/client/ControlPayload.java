package dev.pexskate.client;

import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.codec.ByteBufCodecs;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;

public record ControlPayload(byte version, byte flags, byte forward, byte steer) implements CustomPacketPayload {
    public static final Type<ControlPayload> TYPE = new Type<>(Identifier.fromNamespaceAndPath("pexskate", "control"));
    public static final StreamCodec<RegistryFriendlyByteBuf, ControlPayload> CODEC = StreamCodec.composite(
            ByteBufCodecs.BYTE, ControlPayload::version,
            ByteBufCodecs.BYTE, ControlPayload::flags,
            ByteBufCodecs.BYTE, ControlPayload::forward,
            ByteBufCodecs.BYTE, ControlPayload::steer,
            ControlPayload::new
    );

    @Override
    public Type<? extends CustomPacketPayload> type() {
        return TYPE;
    }
}
