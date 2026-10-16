package dev.pexskate.client;

import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.codec.ByteBufCodecs;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;

/** Advertises the client protocol before a player can enable Skate mode. */
public record ClientHelloPayload(byte version) implements CustomPacketPayload {
    public static final Type<ClientHelloPayload> TYPE = new Type<>(
            Identifier.fromNamespaceAndPath("pexskate", "hello"));
    public static final StreamCodec<RegistryFriendlyByteBuf, ClientHelloPayload> CODEC =
            StreamCodec.composite(ByteBufCodecs.BYTE, ClientHelloPayload::version, ClientHelloPayload::new);

    @Override
    public Type<? extends CustomPacketPayload> type() {
        return TYPE;
    }
}
