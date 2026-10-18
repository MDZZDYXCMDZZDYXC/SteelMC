use std::io::Cursor;
use std::sync::Arc;

use steel_protocol::packet_traits::{CompressionInfo, EncodedPacket};
use steel_registry::packets::play::{C_DAMAGE_EVENT, C_HURT_ANIMATION};
use steel_registry::{init_vanilla_registry, vanilla_damage_types};
use steel_utils::ChunkPos;
use steel_utils::codec::VarInt;
use steel_utils::locks::SyncMutex;
use steel_utils::serial::ReadFrom;
use text_components::TextComponent;

use crate::entity::damage::DamageSource;
use crate::player::connection::NetworkConnection;
use crate::player::{Player, PlayerConnection, ResetReason};
use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
use crate::world::World;

struct RecordingConnection {
    packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
}

impl NetworkConnection for RecordingConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }

    fn send_encoded(&self, packet: EncodedPacket) {
        self.packets.lock().push(packet);
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        self.packets.lock().extend(packets);
    }

    fn disconnect_with_reason(&self, _reason: TextComponent) {}

    fn tick(&self) {}

    fn latency(&self) -> i32 {
        0
    }

    fn close(&self) {}

    fn closed(&self) -> bool {
        false
    }
}

/// Builds a player that tracks chunk `(0, 0)` so nearby hurt broadcasts reach it.
fn tracked_player(world: &Arc<World>) -> (Arc<Player>, Arc<SyncMutex<Vec<EncodedPacket>>>) {
    let packets = Arc::new(SyncMutex::new(Vec::new()));
    let connection = Arc::new(PlayerConnection::Other(Box::new(RecordingConnection {
        packets: Arc::clone(&packets),
    })));
    let player = TestPlayerBuilder::new(Arc::clone(world), "HurtObserver", 1)
        .connection(connection)
        .build();
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    let _ = player.mark_joined_world();
    player.set_client_loaded(true);
    player
        .chunk_sender()
        .lock()
        .mark_chunk_sent_for_test(ChunkPos::new(0, 0));
    packets.lock().clear();
    (player, packets)
}

fn packet_ids(packets: &SyncMutex<Vec<EncodedPacket>>) -> Vec<i32> {
    packets
        .lock()
        .iter()
        .map(|packet| {
            let mut cursor = Cursor::new(packet.encoded_data.as_slice());
            VarInt::read(&mut cursor).expect("packet length should decode");
            VarInt::read(&mut cursor)
                .expect("packet id should decode")
                .0
        })
        .collect()
}

#[test]
fn fall_damage_broadcasts_no_hurt_animation() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("hurt_animation_fall");
    let world = &world_fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let (player, packets) = tracked_player(world);

    assert!(player.hurt(
        world,
        &DamageSource::environment(&vanilla_damage_types::FALL),
        2.0
    ));

    let ids = packet_ids(&packets);
    assert_eq!(
        ids.iter().filter(|id| **id == C_DAMAGE_EVENT).count(),
        1,
        "one fall should broadcast a single damage event"
    );
    assert_eq!(
        ids.iter().filter(|id| **id == C_HURT_ANIMATION).count(),
        0,
        "fall is no_knockback, so the client must keep the tilt started by the damage event"
    );

    world.remove_player_for_world_change(&player);
}

#[test]
fn knockback_damage_broadcasts_the_hurt_animation() {
    init_vanilla_registry();
    let world_fixture = fresh_test_world("hurt_animation_attack");
    let world = &world_fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let (player, packets) = tracked_player(world);

    assert!(player.hurt(
        world,
        &DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK),
        2.0
    ));

    let ids = packet_ids(&packets);
    assert_eq!(
        ids.iter().filter(|id| **id == C_DAMAGE_EVENT).count(),
        1,
        "one hit should broadcast a single damage event"
    );
    assert_eq!(
        ids.iter().filter(|id| **id == C_HURT_ANIMATION).count(),
        1,
        "damage that applies knockback also animates the hurt tilt"
    );

    world.remove_player_for_world_change(&player);
}
