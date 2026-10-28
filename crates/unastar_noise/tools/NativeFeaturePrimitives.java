/* Pinned Java 1.21.11 feature consumer captures. Uses NativeWorldgenOracle's
 * mapping and tagged-registry bootstrap without changing its implementation.
 * Run featureprobe.NativeFeaturePrimitives with one processor and at most 1 GiB.
 * No server lifecycle, sockets, ticks, or hand-translated feature algorithms.
 */
package featureprobe;
import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import com.google.gson.Gson;

public final class NativeFeaturePrimitives {
    static final String MC = "net.minecraft.", LG = MC + "world.level.levelgen.", CH = MC + "world.level.chunk.";
    static Object oracle;
    static Object invoke(String name, Class<?>[] signature, Object... args) throws Exception {
        Method m = oracle.getClass().getDeclaredMethod(name, signature);
        m.setAccessible(true);
        return m.invoke(oracle, args);
    }
    static Class<?> type(String name) throws Exception { return (Class<?>) invoke("type", new Class[]{String.class}, name); }
    static Object call(String owner, String name, Object receiver, String[] signature, Object... args) throws Exception {
        return invoke("call", new Class[]{String.class, String.class, Object.class, String[].class, Object[].class}, owner, name, receiver, signature, args);
    }
    static Method method(String owner, String name, String... signature) throws Exception {
        return (Method) invoke("method", new Class[]{String.class, String.class, String[].class}, owner, name, signature);
    }
    static boolean matches(Method actual, String owner, String name, String... signature) throws Exception {
        Method expected = method(owner, name, signature);
        return actual.getName().equals(expected.getName()) && Arrays.equals(actual.getParameterTypes(), expected.getParameterTypes());
    }
    static Object field(String owner, String name) throws Exception { return invoke("field", new Class[]{String.class, String.class}, owner, name); }
    static Object make(String owner, String[] signature, Object... args) throws Exception {
        return invoke("construct", new Class[]{String.class, String[].class, Object[].class}, owner, signature, args);
    }
    static Object access(Object record, String name) throws Exception {
        Method m = record.getClass().getDeclaredMethod(name); m.setAccessible(true); return m.invoke(record);
    }
    static Object pos(int x, int y, int z) throws Exception { return make(MC + "core.BlockPos", new String[]{"int", "int", "int"}, x, y, z); }
    static int axis(Object pos, String name) throws Exception { return (int) call(MC + "core.Vec3i", name, pos, new String[]{}); }
    static String canonical(Object state) throws Exception { return (String) invoke("canonicalState", new Class[]{Object.class}, state); }

    static final class World implements InvocationHandler {
        final Object state, bounds, factory, material, biome;
        final Map<Long, Object> chunks = new TreeMap<>();
        final Set<String> called = new TreeSet<>();
        World(Object state) throws Exception {
            this.state = state;
            bounds = call(MC + "world.level.LevelHeightAccessor", "create", null, new String[]{"int", "int"}, 0, 256);
            factory = NativeFeaturePrimitives.invoke("paletteFactory", new Class[]{state.getClass()}, state);
            material = call(MC + "world.level.block.Block", "defaultBlockState", field(MC + "world.level.block.Blocks", "NETHERRACK"), new String[]{});
            Object registry = NativeFeaturePrimitives.invoke("registry", new Class[]{Object.class, String.class}, access(state, "lookup"), "BIOME");
            Object location = call(MC + "resources.Identifier", "parse", null, new String[]{String.class.getName()}, "minecraft:nether_wastes");
            Object key = call(MC + "resources.ResourceKey", "create", null, new String[]{MC + "resources.ResourceKey", MC + "resources.Identifier"}, field(MC + "core.registries.Registries", "BIOME"), location);
            biome = call(MC + "core.HolderGetter", "getOrThrow", registry, new String[]{MC + "resources.ResourceKey"}, key);
        }
        Object chunk(int x, int z) throws Exception {
            long key = ((long)x << 32) ^ (z & 0xffffffffL);
            if (chunks.containsKey(key)) return chunks.get(key);
            Object cp = make(MC + "world.level.ChunkPos", new String[]{"int", "int"}, x, z);
            Object chunk = make(CH + "ProtoChunk", new String[]{MC + "world.level.ChunkPos", CH + "UpgradeData", MC + "world.level.LevelHeightAccessor", CH + "PalettedContainerFactory", LG + "blending.BlendingData"}, cp, field(CH + "UpgradeData", "EMPTY"), bounds, factory, null);
            Method put = method(CH + "ChunkAccess", "setBlockState", MC + "core.BlockPos", MC + "world.level.block.state.BlockState");
            for (int y=0;y<128;y++) for (int bz=0;bz<16;bz++) for (int bx=0;bx<16;bx++) put.invoke(chunk, pos(x*16+bx,y,z*16+bz), material);
            chunks.put(key, chunk);
            return chunk;
        }
        public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
            called.add(m.toString());
            if (matches(m, MC + "world.level.LevelHeightAccessor", "getMinY")) return 0;
            if (matches(m, MC + "world.level.LevelHeightAccessor", "getHeight")) return 256;
            if (matches(m, MC + "world.level.WorldGenLevel", "ensureCanWrite", MC + "core.BlockPos")) return true;
            if (matches(m, MC + "world.level.LevelReader", "getHeight", LG + "Heightmap$Types", "int", "int")) return 128;
            if (matches(m, MC + "world.level.LevelReader", "getBiome", MC + "core.BlockPos")) return biome;
            if (matches(m, MC + "world.level.BlockGetter", "getBlockState", MC + "core.BlockPos")) {
                Object p=args[0]; return call(CH + "ProtoChunk", "getBlockState", chunk(axis(p,"getX")>>4,axis(p,"getZ")>>4), new String[]{MC + "core.BlockPos"},p);
            }
            if (matches(m, MC + "world.level.LevelReader", "getChunk", "int", "int", CH + "status.ChunkStatus", "boolean")) return chunk((int)args[0],(int)args[1]);
            if (m.isDefault()) return InvocationHandler.invokeDefault(proxy,m,args);
            throw new UnsupportedOperationException("Uncaptured native world method " + m);
        }
    }
    static Object capture(Object state, Object generator, long seed, int cx, int cz) throws Exception {
        World handler = new World(state);
        Object world = Proxy.newProxyInstance(NativeFeaturePrimitives.class.getClassLoader(), new Class[]{type(MC + "world.level.WorldGenLevel")}, handler);
        Object registry = invoke("registry", new Class[]{Object.class,String.class}, access(state,"lookup"), "PLACED_FEATURE");
        Object location = call(MC + "resources.Identifier","parse",null,new String[]{"java.lang.String"},"minecraft:ore_quartz_nether");
        Object key = call(MC + "resources.ResourceKey","create",null,new String[]{MC+"resources.ResourceKey",MC+"resources.Identifier"},field(MC+"core.registries.Registries","PLACED_FEATURE"),location);
        Object holder = call(MC+"core.HolderGetter","getOrThrow",registry,new String[]{MC+"resources.ResourceKey"},key);
        Object placed = call(MC+"core.Holder","value",holder,new String[]{});
        Object rng = make(LG+"WorldgenRandom",new String[]{MC+"util.RandomSource"},make(LG+"XoroshiroRandomSource",new String[]{"long"},0L));
        long decoration = (long)call(LG+"WorldgenRandom","setDecorationSeed",rng,new String[]{"long","int","int"},seed,cx*16,cz*16);
        call(LG+"WorldgenRandom","setFeatureSeed",rng,new String[]{"long","int","int"},decoration,20,7);
        boolean success=(boolean)call(LG+"placement.PlacedFeature","placeWithBiomeCheck",placed,new String[]{MC+"world.level.WorldGenLevel",MC+"world.level.chunk.ChunkGenerator",MC+"util.RandomSource",MC+"core.BlockPos"},world,generator,rng,pos(cx*16,0,cz*16));
        int continuation=(int)call(MC+"util.RandomSource","nextInt",rng,new String[]{});
        List<Object> writes=new ArrayList<>();
        Method get=method(CH+"ProtoChunk","getBlockState",MC+"core.BlockPos");
        for(var e:handler.chunks.entrySet()) {
            int x=(int)(e.getKey()>>32),z=(int)(long)e.getKey();
            for(int y=0;y<128;y++)for(int bz=0;bz<16;bz++)for(int bx=0;bx<16;bx++) {
                int wx=x*16+bx,wz=z*16+bz;
                Object value=get.invoke(e.getValue(),pos(wx,y,wz));
                if(!value.equals(handler.material)) writes.add(Map.of("x",wx,"y",y,"z",wz,"state",canonical(value)));
            }
        }
        return Map.of("seed",seed,"chunk_x",cx,"chunk_z",cz,"decoration_seed",decoration,"feature_index",20,"step",7,"success",success,"next_int",continuation,"blocks",writes);
    }
    public static void main(String[] args) throws Exception {
        Path runtime=Path.of(args[0]);
        java.net.URL source=NativeFeaturePrimitives.class.getProtectionDomain().getCodeSource().getLocation();
        ClassLoader loader=new java.net.URLClassLoader(new java.net.URL[]{source},NativeFeaturePrimitives.class.getClassLoader()) {
            @Override protected Class<?> loadClass(String name,boolean resolve) throws ClassNotFoundException {
                if(!name.equals("NativeWorldgenOracle")&&!name.startsWith("NativeWorldgenOracle$"))return super.loadClass(name,resolve);
                Class<?> value=findLoadedClass(name);if(value==null)value=findClass(name);if(resolve)resolveClass(value);return value;
            }
        };
        Constructor<?> ctor=Class.forName("NativeWorldgenOracle",true,loader).getDeclaredConstructor(Path.class);ctor.setAccessible(true);oracle=ctor.newInstance(runtime.resolve("server-mappings.txt"));
        List<Object> cases=new ArrayList<>();
        for(long seed:new long[]{0L,12345L,Long.MIN_VALUE}) {
            Object state=invoke("bootstrap",new Class[]{long.class,String.class,boolean.class},seed,"nether",true);
            Object generator=make(LG+"NoiseBasedChunkGenerator",new String[]{MC+"world.level.biome.BiomeSource",MC+"core.Holder"},access(state,"source"),access(state,"settingsHolder"));
            for(int[] p:new int[][]{{0,0},{-1,1}})cases.add(capture(state,generator,seed,p[0],p[1]));
        }
        Files.writeString(Path.of(args[1]),new Gson().toJson(Map.of("version","1.21.11","profile","actual tagged vanilla placed ore_quartz_nether; isolated all-netherrack y0..127, air128..255; native dimension build height256/generator depth128; no neighboring feature history","cases",cases)));
    }
}
