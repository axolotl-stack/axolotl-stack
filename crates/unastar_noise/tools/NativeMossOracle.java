/* Pinned Java 1.21.11 vegetation consumer measurements. This is an isolated
 * primitive fixture, NOT a natural-world FEATURES parity reference. Native
 * tagged configurations, algorithms and random sources are used unchanged.
 * No server lifecycle, network, ticks, or translated feature algorithm.
 */
package featureprobe;

import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import com.google.gson.Gson;
import static featureprobe.NativeFeaturePrimitives.*;

public final class NativeMossOracle {
    static Object registered(Object state, String registryName, String id) throws Exception {
        Object registry = NativeFeaturePrimitives.invoke("registry", new Class[]{Object.class, String.class}, access(state, "lookup"), registryName);
        Object location = call(MC + "resources.Identifier", "parse", null, new String[]{"java.lang.String"}, id);
        Object key = call(MC + "resources.ResourceKey", "create", null, new String[]{MC + "resources.ResourceKey", MC + "resources.Identifier"}, field(MC + "core.registries.Registries", registryName), location);
        Object holder = call(MC + "core.HolderGetter", "getOrThrow", registry, new String[]{MC + "resources.ResourceKey"}, key);
        return call(MC + "core.Holder", "value", holder, new String[]{});
    }
    static List<Integer> xyz(Object p) throws Exception {
        return List.of(axis(p, "getX"), axis(p, "getY"), axis(p, "getZ"));
    }
    static final class Cave implements InvocationHandler {
        final Object air, stone;
        final Map<List<Integer>, Object> blocks = new HashMap<>();
        final List<Object> writes = new ArrayList<>();
        final boolean uneven;
        Cave(boolean uneven) throws Exception {
            this.uneven = uneven;
            air = call(MC + "world.level.block.Block", "defaultBlockState", field(MC + "world.level.block.Blocks", "AIR"), new String[]{});
            stone = call(MC + "world.level.block.Block", "defaultBlockState", field(MC + "world.level.block.Blocks", "DEEPSLATE"), new String[]{});
        }
        Object get(Object p) throws Exception {
            var key = xyz(p);
            Object value = blocks.get(key);
            if (value != null) return value;
            int floor = -48, ceiling = -40;
            if (uneven) { floor += Math.floorMod(key.get(0) * 3 + key.get(2), 5); ceiling += Math.floorMod(key.get(0) + key.get(2) * 2, 3); }
            return key.get(1) <= floor || key.get(1) >= ceiling ? stone : air;
        }
        public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
            if (matches(m, MC + "world.level.LevelHeightAccessor", "getMinY")) return -64;
            if (matches(m, MC + "world.level.LevelHeightAccessor", "getHeight")) return 384;
            if (matches(m, MC + "world.level.WorldGenLevel", "ensureCanWrite", MC + "core.BlockPos")) return true;
            if (matches(m, MC + "world.level.BlockGetter", "getBlockState", MC + "core.BlockPos")) return get(args[0]);
            if (matches(m, MC + "world.level.LevelSimulatedReader", "isStateAtPosition", MC + "core.BlockPos", "java.util.function.Predicate"))
                return ((java.util.function.Predicate<Object>)args[1]).test(get(args[0]));
            if (matches(m, MC + "world.level.LevelWriter", "setBlock", MC + "core.BlockPos", MC + "world.level.block.state.BlockState", "int", "int")) {
                var p = xyz(args[0]); blocks.put(p, args[1]); writes.add(Map.of("pos", p, "state", canonical(args[1]))); return true;
            }
            if (matches(m, MC + "world.level.BlockGetter", "getFluidState", MC + "core.BlockPos"))
                return call(MC + "world.level.block.state.BlockBehaviour$BlockStateBase", "getFluidState", get(args[0]), new String[]{});
            if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
            throw new UnsupportedOperationException("Uncaptured native world method " + m);
        }
    }
    static Object member(Object cfg, String name) throws Exception {
        return NativeFeaturePrimitives.invoke("instanceField", new Class[]{String.class, String.class, Object.class}, LG + "feature.configurations.VegetationPatchConfiguration", name, cfg);
    }
    static Object capture(Object state, Object generator, String id, long seed, boolean uneven) throws Exception {
        Cave cave = new Cave(uneven);
        Object world = Proxy.newProxyInstance(NativeMossOracle.class.getClassLoader(), new Class[]{type(MC + "world.level.WorldGenLevel")}, cave);
        Object random = make(LG + "WorldgenRandom", new String[]{MC + "util.RandomSource"}, make(LG + "XoroshiroRandomSource", new String[]{"long"}, seed));
        boolean uniformDepth = id.equals("uniform_depth_probe");
        Object feature = registered(state, "CONFIGURED_FEATURE", uniformDepth ? "minecraft:moss_patch" : id);
        if (uniformDepth) {
            Object cfg = call(LG + "feature.ConfiguredFeature", "config", feature, new String[]{});
            Object depth = call(MC + "util.valueproviders.UniformInt", "of", null, new String[]{"int", "int"}, 1, 3);
            Object modified = make(LG + "feature.configurations.VegetationPatchConfiguration",
                new String[]{MC + "tags.TagKey", LG + "feature.stateproviders.BlockStateProvider", MC + "core.Holder", LG + "placement.CaveSurface", MC + "util.valueproviders.IntProvider", "float", "int", "float", MC + "util.valueproviders.IntProvider", "float"},
                member(cfg, "replaceable"), member(cfg, "groundState"), member(cfg, "vegetationFeature"), member(cfg, "surface"), depth, 0.65f, member(cfg, "verticalRange"), member(cfg, "vegetationChance"), member(cfg, "xzRadius"), member(cfg, "extraEdgeColumnChance"));
            feature = make(LG + "feature.ConfiguredFeature", new String[]{LG + "feature.Feature", LG + "feature.configurations.FeatureConfiguration"},
                call(LG + "feature.ConfiguredFeature", "feature", feature, new String[]{}), modified);
        }
        int y = -42;
        boolean success = (boolean) call(LG + "feature.ConfiguredFeature", "place", feature,
            new String[]{MC + "world.level.WorldGenLevel", CH + "ChunkGenerator", MC + "util.RandomSource", MC + "core.BlockPos"}, world, generator, random, pos(-3, y, -7));
        int continuation = (int) call(MC + "util.RandomSource", "nextInt", random, new String[]{});
        return Map.of("configured_feature", id, "seed", seed, "uneven", uneven, "origin", List.of(-3, y, -7), "success", success, "next_int", continuation, "writes", cave.writes);
    }
    public static void main(String[] args) throws Exception {
        Path runtime = Path.of(args[0]);
        java.net.URL source = NativeMossOracle.class.getProtectionDomain().getCodeSource().getLocation();
        ClassLoader loader = new java.net.URLClassLoader(new java.net.URL[]{source}, NativeMossOracle.class.getClassLoader()) {
            @Override protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
                if (!name.equals("NativeWorldgenOracle") && !name.startsWith("NativeWorldgenOracle$")) return super.loadClass(name, resolve);
                Class<?> value = findLoadedClass(name); if (value == null) value = findClass(name); if (resolve) resolveClass(value); return value;
            }
        };
        Constructor<?> ctor = Class.forName("NativeWorldgenOracle", true, loader).getDeclaredConstructor(Path.class);
        ctor.setAccessible(true); NativeFeaturePrimitives.oracle = ctor.newInstance(runtime.resolve("server-mappings.txt"));
        Object state = NativeFeaturePrimitives.invoke("bootstrap", new Class[]{long.class, String.class, boolean.class}, 12345L, "overworld", true);
        Object generator = make(LG + "NoiseBasedChunkGenerator", new String[]{MC + "world.level.biome.BiomeSource", MC + "core.Holder"}, access(state, "source"), access(state, "settingsHolder"));
        List<Object> cases = new ArrayList<>();
        // The final profile is a labeled native constructor probe, not a
        // registered vanilla identity: moss_patch with depth uniform[1,3]
        // and extraBottomBlockChance .65, all other fields retained.
        for (String id : List.of("minecraft:moss_patch", "minecraft:moss_patch_ceiling", "minecraft:lush_caves_clay", "uniform_depth_probe"))
            for (long seed : new long[]{0L, 12345L, Long.MIN_VALUE}) for (boolean uneven : new boolean[]{false, true})
                cases.add(capture(state, generator, id, seed, uneven));
        Files.writeString(Path.of(args[1]), new Gson().toJson(Map.of("version", "1.21.11", "profile", "ISOLATED native configured vegetation primitives; floor y=-48 ceiling y=-40, deepslate outside/air inside; uneven floor offset floorMod(3*x+z,5), ceiling offset floorMod(x+2*z,3); uniform_depth_probe is the native moss_patch constructor with depth uniform[1,3] and extraBottomBlockChance .65, not a vanilla registry identity; NOT natural-world FEATURES parity", "cases", cases)));
    }
}
