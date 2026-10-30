use unastar::world::chunk::{BlockStateSpace, Chunk, MIN_Y};
use unastar::world::generator::QuartBiomes;
use unastar_noise::java_biomes::JavaBiome;

#[test]
fn end_biomes_remain_distinct_until_final_bedrock_conversion() {
    let highlands = JavaBiome::EndHighlands;
    let barrens = JavaBiome::EndBarrens;
    let mut quarts = QuartBiomes::filled(highlands as u16);
    quarts.set(1, 0, 0, barrens as u16);
    let mut chunk = Chunk::new_in_space(-1, -1, BlockStateSpace::Java);
    chunk.set_biomes_from_quarts(&quarts, u32::from);
    assert_eq!(chunk.biome_at(0, MIN_Y as i16, 0), Some(highlands as u32));
    assert_eq!(chunk.biome_at(4, MIN_Y as i16, 0), Some(barrens as u32));
    assert_ne!(highlands, barrens);
    assert_eq!(highlands.data().bedrock_id, barrens.data().bedrock_id);
    chunk.convert_to_bedrock().unwrap();
    assert_eq!(
        chunk.biome_at(0, MIN_Y as i16, 0),
        Some(highlands.data().bedrock_id)
    );
    assert_eq!(
        chunk.biome_at(4, MIN_Y as i16, 0),
        Some(barrens.data().bedrock_id)
    );
    chunk.convert_to_bedrock().unwrap();
    assert_eq!(
        chunk.biome_at(4, MIN_Y as i16, 0),
        Some(barrens.data().bedrock_id)
    );
}
