/* Exact native placed-feature observer plus isolated replay. The original
 * native call is never replaced. Replay uses native copied sections and a
 * cloned RNG; biome reads delegate the unchanged original stored-biome view.
 */
package mossprobe;

import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import java.util.stream.Stream;
import com.google.gson.Gson;

public final class NativeMossFeatureProbe implements NativeMossAgent.Observer {
    static final String MC="net.minecraft.", LG=MC+"world.level.levelgen.", CH=MC+"world.level.chunk.";
    final NativeMossTaskProbe api;
    final Map<Object,String> names=new IdentityHashMap<>();
    final Deque<Boolean> calls=new ArrayDeque<>();
    final Deque<Map<String,Object>> pendingWrites=new ArrayDeque<>();
    final Deque<List<Integer>> pendingBiomes=new ArrayDeque<>();
    Trace active;
    boolean replaying;
    static final class Trace {
        Object feature,world,generator,origin,random;
        Object[] rngBefore,rngAfter;
        int minY,height,chunkX,chunkZ,writeRadius;
        long zoomSeed;
        final Map<Long,Object[]> sections=new HashMap<>();
        final List<Object> substrate=new ArrayList<>(),writes=new ArrayList<>(),biomes=new ArrayList<>();
    }
    public NativeMossFeatureProbe(NativeMossTaskProbe api){this.api=api;NativeMossAgent.observer=this;}
    Object call(String owner,String name,Object receiver,String[] signature,Object...args)throws Exception{return api.call(owner,name,receiver,signature,args);}
    Object make(String owner,String[] signature,Object...args)throws Exception{return api.invoke("construct",new Class[]{String.class,String[].class,Object[].class},owner,signature,args);}
    Object field(String owner,String name)throws Exception{return api.invoke("field",new Class[]{String.class,String.class},owner,name);}
    Class<?> type(String name)throws Exception{return (Class<?>)api.invoke("type",new Class[]{String.class},name);}
    Method method(String owner,String name,String...signature)throws Exception{return (Method)api.invoke("method",new Class[]{String.class,String.class,String[].class},owner,name,signature);}
    boolean matches(Method actual,String owner,String name,String...signature)throws Exception {
        Method expected=method(owner,name,signature);return actual.getName().equals(expected.getName())&&Arrays.equals(actual.getParameterTypes(),expected.getParameterTypes());
    }
    List<Integer> xyz(Object p)throws Exception {
        List<Integer> xyz=new ArrayList<>();for(String axis:List.of("getX","getY","getZ"))xyz.add((int)call(MC+"core.Vec3i",axis,p,new String[]{}));return xyz;
    }
    Object[] randomState(Object random)throws Exception {
        Object source=api.member(LG+"WorldgenRandom","randomSource",random);
        Object bits=api.member(LG+"XoroshiroRandomSource","randomNumberGenerator",source);
        return new Object[]{api.member(LG+"Xoroshiro128PlusPlus","seedLo",bits),api.member(LG+"Xoroshiro128PlusPlus","seedHi",bits),api.member(LG+"WorldgenRandom","count",random)};
    }
    long key(int x,int z){return ((long)x<<32)^(z&0xffffffffL);}
    void nameFeatures(Object world)throws Exception {
        if(!names.isEmpty())return;
        Object access=call(MC+"server.level.WorldGenRegion","registryAccess",world,new String[]{});
        Object registry=call(MC+"core.RegistryAccess","lookupOrThrow",access,new String[]{MC+"resources.ResourceKey"},field(MC+"core.registries.Registries","PLACED_FEATURE"));
        Stream<?> holders=(Stream<?>)call(MC+"core.HolderLookup","listElements",registry,new String[]{});
        for(Object holder:holders.toList())names.put(call(MC+"core.Holder","value",holder,new String[]{}),(String)call(MC+"core.Holder","getRegisteredName",holder,new String[]{}));
    }
    public void placedStart(Object feature,Object world,Object generator,Object random,Object pos)throws Exception {
        if(replaying)return;
        nameFeatures(world);
        Object center=call(CH+"ChunkAccess","getPos",api.member(MC+"server.level.WorldGenRegion","center",world),new String[]{});
        int x=(int)api.member(MC+"world.level.ChunkPos","x",center),z=(int)api.member(MC+"world.level.ChunkPos","z",center);
        boolean selected=x==1&&z==2&&"minecraft:lush_caves_clay".equals(names.get(feature));calls.push(selected);
        if(!selected)return;
        if(active!=null)throw new IllegalStateException("nested selected native feature");
        Trace trace=new Trace();active=trace;
        trace.feature=feature;trace.world=world;trace.generator=generator;trace.origin=pos;trace.random=random;trace.rngBefore=randomState(random);
        trace.chunkX=x;trace.chunkZ=z;
        trace.writeRadius=(int)call(CH+"status.ChunkStep","blockStateWriteRadius",api.member(MC+"server.level.WorldGenRegion","generatingStep",world),new String[]{});
        trace.zoomSeed=(long)api.member(MC+"world.level.biome.BiomeManager","biomeZoomSeed",api.member(MC+"server.level.WorldGenRegion","biomeManager",world));
        trace.minY=(int)call(MC+"world.level.LevelHeightAccessor","getMinY",world,new String[]{});
        trace.height=(int)call(MC+"world.level.LevelHeightAccessor","getHeight",world,new String[]{});
        Object cache=api.member(MC+"server.level.WorldGenRegion","cache",world);
        for(int cz=z-1;cz<=z+1;cz++)for(int cx=x-1;cx<=x+1;cx++) {
            Object holder=call(MC+"util.StaticCache2D","get",cache,new String[]{"int","int"},cx,cz);
            Object chunk=call(MC+"server.level.GenerationChunkHolder","getLatestChunk",holder,new String[]{});
            trace.substrate.add(api.snapshot(chunk));
            Object[] sections=(Object[])call(CH+"ChunkAccess","getSections",chunk,new String[]{}),copies=new Object[sections.length];
            for(int i=0;i<sections.length;i++)copies[i]=call(CH+"LevelChunkSection","copy",sections[i],new String[]{});
            trace.sections.put(key(cx,cz),copies);
        }
    }
    public void placedEnd(boolean result)throws Exception {
        if(replaying)return;
        if(!calls.pop())return;
        Trace trace=active;trace.rngAfter=randomState(trace.random);active=null;
        Map<String,Object> capture=new LinkedHashMap<>();
        capture.put("profile","actual original native lush_caves_clay placeWithBiomeCheck at actual FEATURES origin (1,2), plus isolated original-method replay on native copied pre-call sections and cloned RNG; no original world queries or mutations during replay");
        capture.put("placed_feature",names.get(trace.feature));capture.put("origin",xyz(trace.origin));
        capture.put("seed",12345L);capture.put("write_radius",trace.writeRadius);capture.put("biome_zoom_seed",trace.zoomSeed);
        capture.put("native_success",result);capture.put("native_rng_before",trace.rngBefore);capture.put("native_rng_after",trace.rngAfter);
        capture.put("native_ordered_writes",trace.writes);capture.put("native_biome_queries",trace.biomes);capture.put("pre_feature_chunks",trace.substrate);
        replaying=true;
        try {
            ReplayWorld handler=new ReplayWorld(trace);
            Object world=Proxy.newProxyInstance(NativeMossFeatureProbe.class.getClassLoader(),new Class[]{type(MC+"world.level.WorldGenLevel")},handler);
            Object source=make(LG+"XoroshiroRandomSource",new String[]{"long","long"},trace.rngBefore[0],trace.rngBefore[1]);
            Object rng=make(LG+"WorldgenRandom",new String[]{MC+"util.RandomSource"},source);
            Object[] before=randomState(rng);
            boolean replayResult=(boolean)call(LG+"placement.PlacedFeature","placeWithBiomeCheck",trace.feature,
                new String[]{MC+"world.level.WorldGenLevel",CH+"ChunkGenerator",MC+"util.RandomSource",MC+"core.BlockPos"},world,trace.generator,rng,trace.origin);
            Object[] after=randomState(rng);
            boolean rngEqual=Objects.equals(before[0],trace.rngBefore[0])&&Objects.equals(before[1],trace.rngBefore[1])
                &&Objects.equals(after[0],trace.rngAfter[0])&&Objects.equals(after[1],trace.rngAfter[1])
                &&((Number)after[2]).intValue()-((Number)before[2]).intValue()==((Number)trace.rngAfter[2]).intValue()-((Number)trace.rngBefore[2]).intValue();
            capture.put("replay_success",replayResult);capture.put("replay_rng_before",before);capture.put("replay_rng_after",after);
            capture.put("replay_ordered_writes",handler.writes);capture.put("replay_ordered_writes_exact",trace.writes.equals(handler.writes));
            capture.put("replay_rng_exact",rngEqual);capture.put("replay_success_exact",replayResult==result);
        } catch(Throwable failure) {
            var stack=new java.io.StringWriter();failure.printStackTrace(new java.io.PrintWriter(stack));capture.put("replay_failure",stack.toString());
        } finally {replaying=false;}
        Files.writeString(api.output.resolve("placed-lush-clay-origin1_2.json"),new Gson().toJson(capture));
    }
    public void writeStart(Object world,Object pos,Object state)throws Exception {
        if(active==null||replaying)return;
        Map<String,Object> write=new LinkedHashMap<>();write.put("pos",xyz(pos));write.put("state",api.canonical(state));active.writes.add(write);pendingWrites.push(write);
    }
    public void writeEnd(boolean result){if(active!=null&&!replaying)pendingWrites.pop().put("accepted",result);}
    public void biomeStart(Object pos)throws Exception {if(active!=null&&!replaying)pendingBiomes.push(xyz(pos));}
    public void biomeEnd(Object holder)throws Exception {
        if(active!=null&&!replaying)active.biomes.add(Map.of("pos",pendingBiomes.pop(),"biome",call(MC+"core.Holder","getRegisteredName",holder,new String[]{})));
    }
    final class ReplayWorld implements InvocationHandler {
        final Trace trace;final List<Object> writes=new ArrayList<>();
        final Object biomeManager;
        ReplayWorld(Trace trace)throws Exception {
            this.trace=trace;
            Object source=Proxy.newProxyInstance(NativeMossFeatureProbe.class.getClassLoader(),new Class[]{type(MC+"world.level.biome.BiomeManager$NoiseBiomeSource")},(proxy,m,args)->{
                int qx=(int)args[0],qy=Math.max(trace.minY>>2,Math.min(((trace.minY+trace.height)>>2)-1,(int)args[1])),qz=(int)args[2];
                Object[] sections=trace.sections.get(key(qx>>2,qz>>2));
                if(sections==null)throw new IllegalStateException("native replay queried absent captured biome chunk");
                return call(CH+"LevelChunkSection","getNoiseBiome",sections[((qy<<2)-trace.minY)>>4],new String[]{"int","int","int"},qx&3,qy&3,qz&3);
            });
            biomeManager=make(MC+"world.level.biome.BiomeManager",new String[]{MC+"world.level.biome.BiomeManager$NoiseBiomeSource","long"},source,trace.zoomSeed);
        }
        boolean writable(Object p)throws Exception {
            var pos=xyz(p);
            return Math.abs((pos.get(0)>>4)-trace.chunkX)<=trace.writeRadius
                &&Math.abs((pos.get(2)>>4)-trace.chunkZ)<=trace.writeRadius
                &&pos.get(1)>=trace.minY&&pos.get(1)<trace.minY+trace.height;
        }
        Object get(Object p)throws Exception {
            var pos=xyz(p);Object[] sections=trace.sections.get(key(pos.get(0)>>4,pos.get(2)>>4));
            if(sections==null||pos.get(1)<trace.minY||pos.get(1)>=trace.minY+trace.height)
                return call(MC+"world.level.block.Block","defaultBlockState",field(MC+"world.level.block.Blocks","AIR"),new String[]{});
            return api.sectionState.invoke(sections[(pos.get(1)-trace.minY)>>4],pos.get(0)&15,pos.get(1)&15,pos.get(2)&15);
        }
        public Object invoke(Object proxy,Method m,Object[] args)throws Throwable {
            if(matches(m,MC+"world.level.LevelHeightAccessor","getMinY"))return trace.minY;
            if(matches(m,MC+"world.level.LevelHeightAccessor","getHeight"))return trace.height;
            if(matches(m,MC+"world.level.BlockGetter","getBlockState",MC+"core.BlockPos"))return get(args[0]);
            if(matches(m,MC+"world.level.BlockGetter","getFluidState",MC+"core.BlockPos"))return call(MC+"world.level.block.state.BlockBehaviour$BlockStateBase","getFluidState",get(args[0]),new String[]{});
            if(matches(m,MC+"world.level.LevelSimulatedReader","isStateAtPosition",MC+"core.BlockPos","java.util.function.Predicate"))return ((java.util.function.Predicate<Object>)args[1]).test(get(args[0]));
            if(matches(m,MC+"world.level.WorldGenLevel","ensureCanWrite",MC+"core.BlockPos"))return writable(args[0]);
            if(matches(m,MC+"world.level.LevelReader","getBiome",MC+"core.BlockPos"))return call(MC+"world.level.biome.BiomeManager","getBiome",biomeManager,new String[]{MC+"core.BlockPos"},args[0]);
            if(matches(m,MC+"world.level.LevelWriter","setBlock",MC+"core.BlockPos",MC+"world.level.block.state.BlockState","int","int")) {
                var p=xyz(args[0]);boolean accepted=writable(args[0]);
                writes.add(Map.of("pos",p,"state",api.canonical(args[1]),"accepted",accepted));
                if(accepted)call(CH+"LevelChunkSection","setBlockState",trace.sections.get(key(p.get(0)>>4,p.get(2)>>4))[(p.get(1)-trace.minY)>>4],new String[]{"int","int","int",MC+"world.level.block.state.BlockState"},p.get(0)&15,p.get(1)&15,p.get(2)&15,args[1]);
                return accepted;
            }
            if(m.isDefault())return InvocationHandler.invokeDefault(proxy,m,args);
            throw new UnsupportedOperationException("uncaptured native replay method "+m);
        }
    }
}
