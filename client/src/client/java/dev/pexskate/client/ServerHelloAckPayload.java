package dev.pexskate.client;

import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.codec.ByteBufCodecs;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;

/** Confirms the PEX server accepted the client's protocol advertisement. */
public record ServerHelloAckPayload(byte version) implements CustomPacketPayload {
    public static final Type<ServerHelloAckPayload> TYPE = new Type<>(
            Identifier.fromNamespaceAndPath("pexskate", "hello_ack"));
    public static final StreamCodec<RegistryFriendlyByteBuf, ServerHelloAckPayload> CODEC =
            StreamCodec.composite(ByteBufCodecs.BYTE, ServerHelloAckPayload::version, ServerHelloAckPayload::new);

    @Override
    public Type<? extends CustomPacketPayload> type() {
        return TYPE;
    }
}
