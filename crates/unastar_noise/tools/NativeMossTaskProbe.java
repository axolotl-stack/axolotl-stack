/* Read-only constructor-task diagnostics. The caller passes existing native
 * cache/ChunkAccess objects. No status requests, biome fills, actors or RNG.
 * Bounds select observations only; they never alter native generation.
 */
package mossprobe;

import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import com.google.gson.Gson;

public final class NativeMossTaskProbe {
    static final String MC="net.minecraft.", CH=MC+"world.level.chunk.";
    final Object oracle;
    final Path output;
    final Method sectionState;
    final Map<Long,List<Object>> chunks=new HashMap<>();
    final Map<Long,Map<List<Integer>,String>> before=new HashMap<>();
    final List<Object> events=new ArrayList<>();
    public NativeMossTaskProbe(Object oracle,Path output)throws Exception {
        this.oracle=oracle;this.output=output;Files.createDirectories(output);
        sectionState=(Method)invoke("method",new Class[]{String.class,String.class,String[].class},CH+"LevelChunkSection","getBlockState",new String[]{"int","int","int"});
        if(Boolean.getBoolean("native.mossFeatureProbe"))new NativeMossFeatureProbe(this);
    }
    Object invoke(String name,Class<?>[] signature,Object...args)throws Exception {
        Method m=oracle.getClass().getDeclaredMethod(name,signature);m.setAccessible(true);
        try{return m.invoke(oracle,args);}catch(InvocationTargetException e){throw new IllegalStateException(e.getCause());}
    }
    Object call(String owner,String name,Object receiver,String[] signature,Object...args)throws Exception {
        return invoke("call",new Class[]{String.class,String.class,Object.class,String[].class,Object[].class},owner,name,receiver,signature,args);
    }
    Object member(String owner,String name,Object receiver)throws Exception {
        return invoke("instanceField",new Class[]{String.class,String.class,Object.class},owner,name,receiver);
    }
    String canonical(Object state)throws Exception{return (String)invoke("canonicalState",new Class[]{Object.class},state);}
    int[] chunkPos(Object chunk)throws Exception {
        Object p=call(CH+"ChunkAccess","getPos",chunk,new String[]{});
        return new int[]{(int)member(MC+"world.level.ChunkPos","x",p),(int)member(MC+"world.level.ChunkPos","z",p)};
    }
    Object snapshot(Object chunk)throws Exception {
        int[] p=chunkPos(chunk);
        int minY=(int)call(MC+"world.level.LevelHeightAccessor","getMinY",chunk,new String[]{});
        int height=(int)call(MC+"world.level.LevelHeightAccessor","getHeight",chunk,new String[]{});
        Object[] sections=(Object[])call(CH+"ChunkAccess","getSections",chunk,new String[]{});
        Map<String,Integer> palette=new LinkedHashMap<>(),biomePalette=new LinkedHashMap<>();
        IdentityHashMap<Object,Integer> ids=new IdentityHashMap<>();
        List<Integer> runs=new ArrayList<>(),biomeRuns=new ArrayList<>();
        int previous=-1,count=0;
        for(int y=minY;y<minY+height;y++)for(int z=0;z<16;z++)for(int x=0;x<16;x++) {
            Object state=sectionState.invoke(sections[(y-minY)>>4],x,y&15,z);
            Integer id=ids.get(state);
            if(id==null){id=palette.computeIfAbsent(canonical(state),ignored->palette.size());ids.put(state,id);}
            if(id==previous)count++;else{if(count>0){runs.add(previous);runs.add(count);}previous=id;count=1;}
        }
        runs.add(previous);runs.add(count);previous=-1;count=0;
        for(int y=Math.floorDiv(minY,4);y<Math.floorDiv(minY+height,4);y++)for(int z=0;z<4;z++)for(int x=0;x<4;x++) {
            Object holder=call(CH+"ChunkAccess","getNoiseBiome",chunk,new String[]{"int","int","int"},p[0]*4+x,y,p[1]*4+z);
            String name=(String)call(MC+"core.Holder","getRegisteredName",holder,new String[]{});
            int id=biomePalette.computeIfAbsent(name,ignored->biomePalette.size());
            if(id==previous)count++;else{if(count>0){biomeRuns.add(previous);biomeRuns.add(count);}previous=id;count=1;}
        }
        biomeRuns.add(previous);biomeRuns.add(count);
        Map<String,Object> heightmaps=new TreeMap<>();
        for(Object entry:(Collection<?>)call(CH+"ChunkAccess","getHeightmaps",chunk,new String[]{})) {
            var e=(Map.Entry<?,?>)entry;
            heightmaps.put(((Enum<?>)e.getKey()).name(),call(MC+"world.level.levelgen.Heightmap","getRawData",e.getValue(),new String[]{}));
        }
        return Map.of("x",p[0],"z",p[1],"min_y",minY,"height",height,"palette",palette.keySet(),"blocks",runs,"biome_palette",biomePalette.keySet(),"biomes",biomeRuns,"heightmaps",heightmaps,
            "persisted_status",call(CH+"status.ChunkStatus","getName",call(CH+"ChunkAccess","getPersistedStatus",chunk,new String[]{}),new String[]{}));
    }
    Map<List<Integer>,String> bounded(List<Object> refs)throws Exception {
        Map<List<Integer>,String> result=new LinkedHashMap<>();
        for(Object chunk:refs) {
            int[] p=chunkPos(chunk);
            Object[] sections=(Object[])call(CH+"ChunkAccess","getSections",chunk,new String[]{});
            int minY=(int)call(MC+"world.level.LevelHeightAccessor","getMinY",chunk,new String[]{});
            for(int y=-36;y<=-12;y++)for(int z=Math.max(16,p[1]*16);z<=Math.min(39,p[1]*16+15);z++)for(int x=Math.max(16,p[0]*16);x<=Math.min(39,p[0]*16+15);x++)
                result.put(List.of(x,y,z),canonical(sectionState.invoke(sections[(y-minY)>>4],x&15,y&15,z&15)));
        }
        return result;
    }
    public synchronized void observe(String phase,long invocation,String dimension,String status,Object cache,Object chunk)throws Exception {
        if(!dimension.equals("minecraft:overworld")||!status.equals("minecraft:features"))return;
        int[] p=chunkPos(chunk);
        if(p[0]<0||p[0]>2||p[1]<0||p[1]>2)return;
        if(phase.equals("before_native_task")) {
            List<Object> refs=new ArrayList<>(),snapshots=new ArrayList<>();
            for(int z=p[1]-1;z<=p[1]+1;z++)for(int x=p[0]-1;x<=p[0]+1;x++) {
                Object holder=call(MC+"util.StaticCache2D","get",cache,new String[]{"int","int"},x,z);
                Object existing=call(MC+"server.level.GenerationChunkHolder","getLatestChunk",holder,new String[]{});
                if(existing==null)throw new IllegalStateException("missing existing FEATURES dependency");
                refs.add(existing);snapshots.add(snapshot(existing));
            }
            chunks.put(invocation,refs);before.put(invocation,bounded(refs));
            Files.writeString(output.resolve("before-origin"+p[0]+"_"+p[1]+"-invocation"+invocation+".json"),new Gson().toJson(Map.of("profile","actual already-provided native FEATURES 3x3 substrate; read-only; no extra status requests, biome fills, actor access or RNG","origin_chunk",List.of(p[0],p[1]),"invocation",invocation,"chunks",snapshots)));
        } else if(phase.equals("native_task_future_completed")) {
            List<Object> refs=chunks.remove(invocation);
            if(refs==null)throw new IllegalStateException("missing before substrate");
            Map<List<Integer>,String> old=before.remove(invocation),after=bounded(refs);
            List<Object> delta=new ArrayList<>();
            for(var entry:after.entrySet())if(!entry.getValue().equals(old.get(entry.getKey())))
                delta.add(Map.of("pos",entry.getKey(),"before",old.get(entry.getKey()),"after",entry.getValue()));
            events.add(Map.of("origin_chunk",List.of(p[0],p[1]),"invocation",invocation,"bounded_block_delta",delta));
            Files.writeString(output.resolve("origin-deltas.json"),new Gson().toJson(events));
        }
    }
}
