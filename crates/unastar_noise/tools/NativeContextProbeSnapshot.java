import java.nio.file.*;
import java.math.BigInteger;
import java.util.*;
import com.google.gson.Gson;

/** Canonical full-volume snapshots of actual pinned 1.21.11 native chunks. */
public final class NativeContextProbeSnapshot {
    static final String MC="net.minecraft.";
    static Object call(String owner,String name,Object receiver,String[] types,Object... args)throws Exception{return NativeContextProbe.call(owner,name,receiver,types,args);}
    static Object make(String owner,String[] types,Object... args)throws Exception{return NativeContextProbe.make(owner,types,args);}
    static Object field(String owner,String name)throws Exception{return NativeContextProbe.field(owner,name);}
    static String unicode(String text) {
        for(int i=0;i<text.length();++i) {
            char c=text.charAt(i);
            if(Character.isHighSurrogate(c)) {
                if(++i==text.length() || !Character.isLowSurrogate(text.charAt(i)))throw new IllegalArgumentException("unpaired UTF-16 high surrogate in canonical NBT string");
            } else if(Character.isLowSurrogate(c))throw new IllegalArgumentException("unpaired UTF-16 low surrogate in canonical NBT string");
        }
        return text;
    }
    static Object nbt(Object tag)throws Exception {
        int id=((Number)call(MC+"nbt.Tag","getId",tag,new String[]{})).intValue();
        String[] names={"End","Byte","Short","Int","Long","Float","Double","ByteArray","Str","List","Compound","IntArray","LongArray"};
        if(id<0 || id>=names.length)throw new IllegalArgumentException("unknown native NBT tag id "+id);
        Object value;
        if(id==10){
            List<Object> pairs=new ArrayList<>();
            // Rust stores compound keys as BTreeMap<Vec<u16>, Tag>: Java String order.
            TreeSet<String> keys=new TreeSet<>();
            for(String key:(Set<String>)call(MC+"nbt.CompoundTag","keySet",tag,new String[]{}))keys.add(unicode(key));
            for(String key:keys)pairs.add(List.of(key,nbt(call(MC+"nbt.CompoundTag","get",tag,new String[]{"java.lang.String"},key))));
            value=pairs;
        }else if(id==9){
            List<Object> values=new ArrayList<>();
            int size=(int)call(MC+"nbt.ListTag","size",tag,new String[]{});
            // Native 1.21.11 stores no declared type; empty lists serialize as END.
            // Match native write's compound wrappers for heterogeneous lists, without
            // passing floating payloads through DataOutput's NaN-canonicalizing writes.
            byte element=((Number)call(MC+"nbt.ListTag","identifyRawElementType",tag,new String[]{})).byteValue();
            for(int index=0;index<size;++index){
                Object child=call(MC+"nbt.ListTag","get",tag,new String[]{"int"},index);
                Object serialized=call(MC+"nbt.ListTag","wrapIfNeeded",null,new String[]{"byte",MC+"nbt.Tag"},element,child);
                values.add(nbt(serialized));
            }
            value=List.of(element,values);
        }else if(id==0)value=0;
        else {
            String[] accessors={"","asByte","asShort","asInt","asLong","asFloat","asDouble","asByteArray","asString","","","asIntArray","asLongArray"};
            value=((Optional<?>)call(MC+"nbt.Tag",accessors[id],tag,new String[]{})).orElseThrow();
            if(id==5)value=Integer.toUnsignedLong(Float.floatToRawIntBits((Float)value));
            else if(id==6)value=new BigInteger(Long.toUnsignedString(Double.doubleToRawLongBits((Double)value)));
            else if(id==8)value=unicode((String)value);
        }
        return Map.of("type",names[id],"value",value);
    }
    static List<Object> ticks(Object chunk,String kind,String registry,long gameTime)throws Exception {
        Object packed=call(MC+"world.level.chunk.ChunkAccess","getTicksForSerialization",chunk,new String[]{"long"},gameTime);
        List<?> ticks=(List<?>)call(MC+"world.level.chunk.ChunkAccess$PackedTicks",kind,packed,new String[]{});
        List<Object> result=new ArrayList<>();
        for(Object tick:ticks){
            Object block=call(MC+"world.ticks.SavedTick","type",tick,new String[]{});
            Object position=call(MC+"world.ticks.SavedTick","pos",tick,new String[]{});
            List<Integer> xyz=new ArrayList<>();
            for(String axis:new String[]{"getX","getY","getZ"})xyz.add((int)call(MC+"core.Vec3i",axis,position,new String[]{}));
            result.add(Map.of("type",call(MC+"core.Registry","getKey",field(MC+"core.registries.BuiltInRegistries",registry),new String[]{"java.lang.Object"},block).toString(),
                "position",xyz,"delay",call(MC+"world.ticks.SavedTick","delay",tick,new String[]{}),
                "priority",call(MC+"world.ticks.SavedTick","priority",tick,new String[]{}).toString()));
        }
        return result;
    }
    /** Complete retained quart storage; does not query a generator or biome source. */
    static Map<String,Object> biomeStorage(Object chunk)throws Exception {
        int minY=(int)call(MC+"world.level.LevelHeightAccessor","getMinY",chunk,new String[]{});
        int height=(int)call(MC+"world.level.LevelHeightAccessor","getHeight",chunk,new String[]{});
        Object pos=call(MC+"world.level.chunk.ChunkAccess","getPos",chunk,new String[]{});
        int x0=(int)call(MC+"world.level.ChunkPos","getMinBlockX",pos,new String[]{});
        int z0=(int)call(MC+"world.level.ChunkPos","getMinBlockZ",pos,new String[]{});
        Map<String,Integer> palette=new LinkedHashMap<>();
        IdentityHashMap<Object,Integer> holders=new IdentityHashMap<>();
        List<Integer> runs=new ArrayList<>();
        int previous=-1,count=0;
        var digest=java.security.MessageDigest.getInstance("SHA-256");
        for(int y=Math.floorDiv(minY,4);y<Math.floorDiv(minY+height,4);++y)for(int z=0;z<4;++z)for(int x=0;x<4;++x){
            Object holder=call(MC+"world.level.chunk.ChunkAccess","getNoiseBiome",chunk,new String[]{"int","int","int"},Math.floorDiv(x0,4)+x,y,Math.floorDiv(z0,4)+z);
            Integer id=holders.get(holder);
            if(id==null){
                String name=(String)call(MC+"core.Holder","getRegisteredName",holder,new String[]{});
                id=palette.computeIfAbsent(name,ignored->palette.size());holders.put(holder,id);
            }
            digest.update((byte)(id>>>24));digest.update((byte)(id>>>16));digest.update((byte)(id>>>8));digest.update((byte)(int)id);
            if(id==previous)++count;else{if(count>0){runs.add(previous);runs.add(count);}previous=id;count=1;}
        }
        if(count>0){runs.add(previous);runs.add(count);}
        for(String name:palette.keySet()){digest.update(name.getBytes(java.nio.charset.StandardCharsets.UTF_8));digest.update((byte)0);}
        return Map.of("min_y",minY,"height",height,"order","quart-y,z,x; RLE palette-index,count",
            "palette",new ArrayList<>(palette.keySet()),"rle",runs,"sha256",HexFormat.of().formatHex(digest.digest()));
    }
    static void snapshot(Object chunk,ary server,String dimension,String stage,Path output)throws Exception {
        String ch=MC+"world.level.chunk.",lg=MC+"world.level.levelgen.";
        Object level=call(MC+"server.MinecraftServer","getLevel",server,new String[]{MC+"resources.ResourceKey"},field(MC+"world.level.Level",dimension));
        long gameTime=(long)call(MC+"world.level.LevelAccessor","getGameTime",level,new String[]{});
        boolean proto=((Class<?>)NativeContextProbe.invoke("type",new Class[]{String.class},ch+"ProtoChunk")).isInstance(chunk);
        int minY=(int)call(MC+"world.level.LevelHeightAccessor","getMinY",chunk,new String[]{});
        int height=(int)call(MC+"world.level.LevelHeightAccessor","getHeight",chunk,new String[]{});
        Map<String,Integer> palette=new LinkedHashMap<>(),biomePalette=new LinkedHashMap<>();
        IdentityHashMap<Object,Integer> stateIds=new IdentityHashMap<>();
        List<Integer> blocks=new ArrayList<>();int previous=-1,count=0;
        Object chunkPos=call(ch+"ChunkAccess","getPos",chunk,new String[]{});
        int x0=(int)call(MC+"world.level.ChunkPos","getMinBlockX",chunkPos,new String[]{}),z0=(int)call(MC+"world.level.ChunkPos","getMinBlockZ",chunkPos,new String[]{});
        for(int y=minY;y<minY+height;++y)for(int z=0;z<16;++z)for(int x=0;x<16;++x){
            Object pos=make(MC+"core.BlockPos",new String[]{"int","int","int"},x0+x,y,z0+z);
            Object state=call(MC+"world.level.BlockGetter","getBlockState",chunk,new String[]{MC+"core.BlockPos"},pos);
            Integer id=stateIds.get(state);
            if(id==null){String canonical=(String)NativeContextProbe.invoke("canonicalState",new Class[]{Object.class},state);id=palette.computeIfAbsent(canonical,ignored->palette.size());stateIds.put(state,id);}
            if(id==previous)++count;else{if(count>0){blocks.add(previous);blocks.add(count);}previous=id;count=1;}
        }
        blocks.add(previous);blocks.add(count);
        int[] biomes=new int[height*4];int index=0;
        Object sections=call(ch+"ChunkAccess","getSections",chunk,new String[]{});
        for(int y=minY/4;y<(minY+height)/4;++y)for(int z=0;z<4;++z)for(int x=0;x<4;++x){
            Object section=java.lang.reflect.Array.get(sections,Math.floorDiv(y,4)-Math.floorDiv(minY,16));
            Object holder=call(ch+"LevelChunkSection","getNoiseBiome",section,new String[]{"int","int","int"},x,Math.floorMod(y,4),z);
            String name=(String)call(MC+"core.Holder","getRegisteredName",holder,new String[]{});
            biomes[index++]=biomePalette.computeIfAbsent(name,ignored->biomePalette.size());
        }
        Map<String,Object> heightmaps=new TreeMap<>();
        for(Object object:(Collection<?>)call(ch+"ChunkAccess","getHeightmaps",chunk,new String[]{})){
            var entry=(Map.Entry<?,?>)object;int[] heights=new int[256];
            for(int z=0;z<16;++z)for(int x=0;x<16;++x)heights[z*16+x]=(int)call(lg+"Heightmap","getFirstAvailable",entry.getValue(),new String[]{"int","int"},x,z);
            heightmaps.put((String)call(lg+"Heightmap$Types","getSerializationKey",entry.getKey(),new String[]{}),heights);
        }
        List<Object> actors=new ArrayList<>();
        for(Object pos:(Set<?>)call(ch+"ChunkAccess","getBlockEntitiesPos",chunk,new String[]{})){
            Object tag=call(ch+"ChunkAccess","getBlockEntityNbtForSaving",chunk,new String[]{MC+"core.BlockPos",MC+"core.HolderLookup$Provider"},pos,server.bc());
            actors.add(nbt(tag));
        }
        List<Object> entities=new ArrayList<>();
        if(proto){
            for(Object tag:(List<?>)call(ch+"ProtoChunk","getEntities",chunk,new String[]{}))entities.add(nbt(tag));
        }else{
            for(Object entity:(Iterable<?>)call(MC+"server.level.ServerLevel","getAllEntities",level,new String[]{})){
                Object pos=call(MC+"world.entity.Entity","blockPosition",entity,new String[]{});
                int ex=(int)call(MC+"core.Vec3i","getX",pos,new String[]{}),ez=(int)call(MC+"core.Vec3i","getZ",pos,new String[]{});
                if(Math.floorDiv(ex,16)!=Math.floorDiv(x0,16)||Math.floorDiv(ez,16)!=Math.floorDiv(z0,16))continue;
                Object value=call(MC+"world.level.storage.TagValueOutput","createWithContext",null,new String[]{MC+"util.ProblemReporter",MC+"core.HolderLookup$Provider"},field(MC+"util.ProblemReporter","DISCARDING"),server.bc());
                boolean saved=(boolean)call(MC+"world.entity.Entity","save",entity,new String[]{MC+"world.level.storage.ValueOutput"},value);
                if(saved)entities.add(nbt(call(MC+"world.level.storage.TagValueOutput","buildResult",value,new String[]{})));
            }
        }
        Map<String,Object> row=new LinkedHashMap<>();
        row.put("x",Math.floorDiv(x0,16));row.put("z",Math.floorDiv(z0,16));
        row.put("status",call(ch+"ChunkAccess","getPersistedStatus",chunk,new String[]{}).toString());
        row.put("blocks",blocks);row.put("biomes",biomes);row.put("heightmaps",heightmaps);row.put("block_entities",actors);
        row.put("entity_nbt_separate",entities);row.put("block_ticks_separate",ticks(chunk,"blocks","BLOCK",gameTime));row.put("fluid_ticks_separate",ticks(chunk,"fluids","FLUID",gameTime));
        row.put("entity_container_boundary_separate",proto?"actual ProtoChunk entity NBT container":"actual ServerLevel runtime entities in this chunk saved through native Entity.save; no ticks");
        row.put("native_chunk_container",chunk.getClass().getName());row.put("world_game_time",gameTime);
        Map<String,Object> result=new LinkedHashMap<>();
        result.put("edition","java");result.put("version","1.21.11");result.put("seed",NativeContextProbe.seed);
        result.put("dimension",NativeContextProbe.invoke("dimensionId",new Class[]{String.class},dimension.toLowerCase(Locale.ROOT)));
        result.put("state_encoding","java-canonical-properties-v1");result.put("nbt_encoding","java-typed-raw-ieee-v2");result.put("min_y",minY);result.put("height",height);
        result.put("palette",palette.keySet());result.put("biome_palette",biomePalette.keySet());result.put("chunks",List.of(row));
        result.put("structures",NativeContextProbe.structures);result.put("datapacks",List.of("vanilla"));
        result.put("inputs",NativeContextProbe.inputs(dimension.toLowerCase(Locale.ROOT)));
        result.put("capture",NativeContextProbe.parallelOrigins
            ? "actual native asynchronous status futures submitted as an origin batch; full-volume snapshot after every target future completed; original dependency scheduling retained, no neighboring FEATURES halo closure, higher status, listeners or ticks"
            : stage.equals("FEATURES_HALO_CLOSURE")
                ? "actual native FEATURES snapshot after every possible neighboring FEATURES writer within observed native blockStateWriteRadius completed; constructor-only native server context; no FULL promotion, SPAWN stage, listeners or ticks"
                : "actual native ServerChunkCache getChunk requested "+stage+" using genuine constructor-only DedicatedServer, actual WorldStem/native registries/ServerLevel/WorldGenRegion/StructureManager; no initServer/runServer/listener/ticks; persisted status captured immediately, before any subsequent target request");
        result.put("actor_stage_truth","actual native chunk live block entity and pending NBT containers; getBlockEntityNbtForSaving with actual native server registry access; raw IEEE typed NBT retained, native ListTag wire element type (empty=END); native entities and packed block/fluid ticks captured separately at recorded game time; no ticks executed");
        result.put("actor_container_counts",Map.of("live",((Map<?,?>)NativeContextProbe.member(ch+"ChunkAccess","blockEntities",chunk)).size(),"pending_nbt",((Map<?,?>)NativeContextProbe.member(ch+"ChunkAccess","pendingBlockEntities",chunk)).size()));
        result.put("native_server_tick_count",call(MC+"server.MinecraftServer","getTickCount",server,new String[]{}));
        result.put("query_profile",NativeContextProbe.evidence.get("query_profile"));
        result.put("coordinate_request_order",NativeContextProbe.activeCoordinateOrder);
        result.put("coordinate_request_index",NativeContextProbe.activeCoordinateRequestIndex);
        result.put("native_request_ordinal",NativeContextProbe.activeNativeRequest);
        result.put("native_chunk_object_id",NativeContextProbe.objectId(chunk));
        result.put("context_profile",NativeContextProbe.evidence.get("native_levels_constructed"));
        result.put("requested_stage",stage);
        if(NativeContextProbe.parallelOrigins){
            Map<String,Object> batch=new LinkedHashMap<>((Map<String,Object>)NativeContextProbe.evidence.get("active_parallel_batch"));
            List<Object> schedule=new ArrayList<>();
            synchronized(NativeContextProbe.nativeTaskTrace){
                for(Object entry:NativeContextProbe.nativeTaskTrace){
                    Map<String,Object> event=(Map<String,Object>)entry;
                    String phase=(String)event.get("phase");
                    if(!result.get("dimension").equals(event.get("dimension")) ||
                        !("before_native_task".equals(phase)||"native_task_future_completed".equals(phase)))continue;
                    // Operation metadata only: never feed native block/biome
                    // payloads back into a generator as scheduling inputs.
                    // Completion events let a consumer prove which tasks ran
                    // concurrently instead of assuming start order.
                    Map<String,Object> task=new LinkedHashMap<>();
                    task.put("event_ordinal",event.get("event_ordinal"));task.put("phase",phase);
                    task.put("invocation",event.get("invocation"));task.put("status",event.get("target_status"));
                    task.put("chunk_x",event.get("chunk_x"));task.put("chunk_z",event.get("chunk_z"));
                    task.put("thread_id",event.get("thread_id"));task.put("thread_name",event.get("thread_name"));
                    schedule.add(task);
                }
            }
            batch.put("native_task_schedule",schedule);result.put("parallel_batch",batch);
        }
        if(stage.equals("FEATURES_HALO_CLOSURE")||Set.of("INITIALIZE_LIGHT","LIGHT","SPAWN","FULL").contains(stage))
            result.put("features_halo",NativeContextProbe.evidence.get("features_halo"));
        result.put("native_chunk_context",Map.of("blending_data_absent",call(ch+"ChunkAccess","getBlendingData",chunk,new String[]{})==null,
            "structure_start_count",((Map<?,?>)call(ch+"ChunkAccess","getAllStarts",chunk,new String[]{})).size(),
            "structure_reference_count",((Map<?,?>)call(ch+"ChunkAccess","getAllReferences",chunk,new String[]{})).size()));
        result.put("server_sha1","64bb6d763bed0a9f1d632ec347938594144943ed");result.put("mappings_sha1","5621e9253f05fd57872bbe7f8ddf5f9a7d525955");result.put("inner_sha256","ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc");
        Files.writeString(output,new Gson().toJson(result));
    }
}
