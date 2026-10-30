import java.nio.file.*;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.*;
import com.google.gson.Gson;
import com.mojang.authlib.yggdrasil.YggdrasilAuthenticationService;

/**
 * Constructor-only native measurements. --trace-native-tasks (after --structures)
 * records bounded original-task callbacks, cache identities and stored BIOMES
 * provenance; all original task arguments and futures are retained. New snapshots
 * carry coordinate_request_index (argument-list index, including repetitions) and
 * native_request_ordinal (global native request order). This tool never claims
 * generator conformance and must run under an external process deadline.
 * Optional -Dnative.mossTaskProbe=true additionally loads the separate
 * mossprobe.NativeMossTaskProbe diagnostic (requires --trace-native-tasks).
 * -Dnative.mossFeatureProbe=true plus NativeMossAgent's runtime javaagent
 * captures the original placed-clay call and a separately labeled clone replay.
 * Neither option adds native chunk/status requests or replaces native calls.
 * --extended-stages additionally observes EMPTY through NOISE before the existing
 * sweep, then INITIALIZE_LIGHT/LIGHT/SPAWN/FULL after the FEATURES closure. These are
 * actual native status requests, not relabeled FEATURES snapshots.
 */
public final class NativeContextProbe {
    static final String MC="net.minecraft.";
    static Object oracle;
    static final Map<String,Object> evidence=new LinkedHashMap<>();
    static final List<Object> nativeRequestOrder=new ArrayList<>();
    static final List<Object> nativeTaskTrace=Collections.synchronizedList(new ArrayList<>());
    static volatile int activeNativeRequest=-1;
    static int activeCoordinateRequestIndex=-1;
    /** Request order of the dimension currently being captured. */
    static List<String> activeCoordinateOrder=List.of();
    static Path taskObserverFailureOutput;
    static final int MAX_TASK_EVENTS=100_000, MAX_OBSERVED_OBJECTS=30_000;
    static final IdentityHashMap<Object,Integer> observedObjects=new IdentityHashMap<>();
    static final IdentityHashMap<Object,Long> completedBiomeInvocations=new IdentityHashMap<>();
    static final Set<Object> observedCaches=Collections.newSetFromMap(new IdentityHashMap<>());
    static final List<Object> nativeStageCaches=new ArrayList<>(), nativeBiomeStorage=new ArrayList<>();
    static final List<String> observerFailures=Collections.synchronizedList(new ArrayList<>());
    static long seed;
    static Object mossTaskProbe;
    static Method mossTaskObserve;
    static boolean structures=false;
    static boolean parallelOrigins;
    static Object inputs(String lowerDimension)throws Exception {
        return invoke("declaredInputs",new Class[]{long.class,String.class,boolean.class},seed,lowerDimension,structures);
    }
    static Object invoke(String name, Class<?>[] types, Object... args) throws Exception {
        Method method=oracle.getClass().getDeclaredMethod(name,types); method.setAccessible(true);
        try { return method.invoke(oracle,args); } catch(InvocationTargetException error) { throw new IllegalStateException(error.getCause()); }
    }
    static Object call(String owner,String name,Object receiver,String[] parameters,Object... args)throws Exception {
        return invoke("call",new Class[]{String.class,String.class,Object.class,String[].class,Object[].class},owner,name,receiver,parameters,args);
    }
    static Object field(String owner,String name)throws Exception { return invoke("field",new Class[]{String.class,String.class},owner,name); }
    static Object make(String owner,String[] parameters,Object... args)throws Exception {return invoke("construct",new Class[]{String.class,String[].class,Object[].class},owner,parameters,args);}
    static Object requestChunk(Object level,String dimension,int x,int z,String status)throws Exception {
        nativeRequestOrder.add(Map.of("ordinal",nativeRequestOrder.size(),"dimension",dimension,"chunk_x",x,"chunk_z",z,"status",status));
        activeNativeRequest=nativeRequestOrder.size()-1;
        return call(MC+"world.level.Level","getChunk",level,new String[]{"int","int",MC+"world.level.chunk.status.ChunkStatus","boolean"},x,z,field(MC+"world.level.chunk.status.ChunkStatus",status),true);
    }
    /** Submit the complete origin batch before pumping native generation work. */
    static void parallelStage(Object level,ary server,String dimension,String stage,
        List<String> coordinates,List<String> captures,Path output,Map<String,Object> structureProofs)throws Exception {
        Object source=call(MC+"server.level.ServerLevel","getChunkSource",level,new String[]{});
        Object status=field(MC+"world.level.chunk.status.ChunkStatus",stage);
        List<CompletableFuture<?>> futures=new ArrayList<>();
        List<Integer> ordinals=new ArrayList<>();
        for(String coordinate:coordinates){
            String[] parts=coordinate.split(":");int x=Integer.parseInt(parts[0]),z=Integer.parseInt(parts[1]);
            int ordinal=nativeRequestOrder.size();ordinals.add(ordinal);
            nativeRequestOrder.add(Map.of("ordinal",ordinal,"dimension",dimension,"chunk_x",x,"chunk_z",z,
                "status",stage,"submission","parallel-origin-batch"));
            activeNativeRequest=ordinal;
            futures.add((CompletableFuture<?>)call(MC+"server.level.ServerChunkCache","getChunkFutureMainThread",
                source,new String[]{"int","int",MC+"world.level.chunk.status.ChunkStatus","boolean"},x,z,status,true));
        }
        long pending=futures.stream().filter(future->!future.isDone()).count();
        if(stage.equals("SURFACE")&&coordinates.size()>1&&pending<2)
            throw new IllegalStateException("native batch did not establish two outstanding status futures");
        CompletableFuture<Void> completed=CompletableFuture.allOf(futures.toArray(CompletableFuture[]::new));
        Object executor=member(MC+"server.level.ServerChunkCache","mainThreadProcessor",source);
        call(MC+"server.level.ServerChunkCache$MainThreadExecutor","managedBlock",executor,
            new String[]{"java.util.function.BooleanSupplier"},(java.util.function.BooleanSupplier)completed::isDone);
        completed.join();
        Map<String,Object> batch=Map.of("dimension",dimension,"status",stage,"request_ordinals",ordinals,
            "coordinate_request_order",List.copyOf(coordinates),
            "pending_futures_before_native_pump",pending,"generation_worker_count",2,
            "observation_boundary","every target status future completed; not a neighboring FEATURES halo closure");
        evidence.put("active_parallel_batch",batch);
        for(int index=0;index<coordinates.size();index++){
            activeCoordinateRequestIndex=index;activeNativeRequest=ordinals.get(index);
            Object result=futures.get(index).join();
            Object chunk=call(MC+"server.level.ChunkResult","orElse",result,new String[]{"java.lang.Object"},(Object)null);
            if(chunk==null)throw new IllegalStateException("native parallel chunk failed: "+
                call(MC+"server.level.ChunkResult","getError",result,new String[]{}));
            String coordinate=coordinates.get(index).replace(':','_');
            Path capture=output.getParent().resolve("parallel-"+dimension.toLowerCase(Locale.ROOT)+"-"+
                stage.toLowerCase(Locale.ROOT)+"-seed"+seed+"-chunk"+coordinate+"-request"+activeNativeRequest+".json");
            NativeContextProbeSnapshot.snapshot(chunk,server,dimension,stage,capture);captures.add(capture.toString());
            if(structures)structureProofs.put(capture.getFileName().toString(),structureProof(chunk,level,server));
        }
        evidence.remove("active_parallel_batch");
    }
    /**
     * Native locator results per dimension: the start chunk of each listed
     * structure nearest to each search origin, with that start's complete piece
     * list (requested at STRUCTURE_STARTS), and the nearest chunk of every biome
     * the dimension's biome source can produce. Run in a context that captures
     * nothing, because locator queries advance native biome tie history.
     */
    static Map<String,Object> discoverAnchors(Map<Object,Object> levels,ary server)throws Exception {
        Map<String,String[]> structureKeys=new LinkedHashMap<>();
        structureKeys.put("OVERWORLD",new String[]{"stronghold","ancient_city","trial_chambers","mansion","monument",
            "village_plains","village_desert","village_savanna","village_snowy","village_taiga","desert_pyramid",
            "jungle_pyramid","pillager_outpost","igloo","swamp_hut","shipwreck","shipwreck_beached","ocean_ruin_cold",
            "ocean_ruin_warm","ruined_portal","ruined_portal_desert","ruined_portal_jungle","ruined_portal_mountain",
            "ruined_portal_ocean","ruined_portal_swamp","buried_treasure","mineshaft","mineshaft_mesa","trail_ruins"});
        structureKeys.put("NETHER",new String[]{"fortress","bastion_remnant","ruined_portal_nether","nether_fossil"});
        structureKeys.put("END",new String[]{"end_city"});
        Object structureRegistry=call(MC+"core.RegistryAccess","lookupOrThrow",server.bc(),new String[]{MC+"resources.ResourceKey"},field(MC+"core.registries.Registries","STRUCTURE"));
        Class<?> holderType=(Class<?>)invoke("type",new Class[]{String.class},MC+"core.Holder");
        Object origin=make(MC+"core.BlockPos",new String[]{"int","int","int"},0,0,0);
        int[][] searchOrigins={{0,0},{4096,4096},{-4096,4096},{4096,-4096},{-4096,-4096}};
        Map<String,Object> result=new LinkedHashMap<>();
        for(String dimension:new String[]{"OVERWORLD","NETHER","END"}){
            Object level=levels.get(field(MC+"world.level.Level",dimension));
            Object source=call(MC+"server.level.ServerLevel","getChunkSource",level,new String[]{});
            Object generator=call(MC+"server.level.ServerChunkCache","getGenerator",source,new String[]{});
            List<Object> structuresFound=new ArrayList<>(),biomesFound=new ArrayList<>();
            for(String key:structureKeys.get(dimension)){
                Object resourceKey=call(MC+"resources.ResourceKey","create",null,new String[]{MC+"resources.ResourceKey",MC+"resources.Identifier"},
                    field(MC+"core.registries.Registries","STRUCTURE"),call(MC+"resources.Identifier","parse",null,new String[]{"java.lang.String"},"minecraft:"+key));
                Object holder=call(MC+"core.HolderGetter","getOrThrow",structureRegistry,new String[]{MC+"resources.ResourceKey"},resourceKey);
                Object holders=java.lang.reflect.Array.newInstance(holderType,1);java.lang.reflect.Array.set(holders,0,holder);
                Object set=call(MC+"core.HolderSet","direct",null,new String[]{MC+"core.Holder[]"},holders);
                Set<String> seenStarts=new HashSet<>();
                for(int[] searchOrigin:searchOrigins){
                    Object from=make(MC+"core.BlockPos",new String[]{"int","int","int"},searchOrigin[0],0,searchOrigin[1]);
                    Object found=call(MC+"world.level.chunk.ChunkGenerator","findNearestMapStructure",generator,
                        new String[]{MC+"server.level.ServerLevel",MC+"core.HolderSet",MC+"core.BlockPos","int","boolean"},level,set,from,100,false);
                    Map<String,Object> row=new LinkedHashMap<>();row.put("structure","minecraft:"+key);row.put("search_origin",List.of(searchOrigin[0],searchOrigin[1]));
                    if(found!=null){
                        List<Integer> location=position(((com.mojang.datafixers.util.Pair<?,?>)found).getFirst());
                        int cx=Math.floorDiv(location.get(0),16),cz=Math.floorDiv(location.get(2),16);
                        if(!seenStarts.add(cx+":"+cz))continue;
                        row.put("block_position",location);row.put("chunk",cx+":"+cz);
                        Object chunk=requestChunk(level,dimension,cx,cz,"STRUCTURE_STARTS");
                        List<Object> starts=new ArrayList<>();
                        for(Object start:(List<?>)((Map<?,?>)structureProof(chunk,level,server)).get("starts"))
                            if(((Map<?,?>)start).get("structure").equals("minecraft:"+key))starts.add(start);
                        row.put("starts",starts);
                    }
                    structuresFound.add(row);
                }
            }
            Object biomeSource=call(MC+"world.level.chunk.ChunkGenerator","getBiomeSource",generator,new String[]{});
            List<Object> possible=new ArrayList<>((Set<?>)call(MC+"world.level.biome.BiomeSource","possibleBiomes",biomeSource,new String[]{}));
            for(Object biome:possible){
                Object key=((Optional<?>)call(MC+"core.Holder","unwrapKey",biome,new String[]{})).orElseThrow();
                String name=call(MC+"resources.ResourceKey","identifier",key,new String[]{}).toString();
                Object found=call(MC+"server.level.ServerLevel","findClosestBiome3d",level,
                    new String[]{"java.util.function.Predicate",MC+"core.BlockPos","int","int","int"},
                    (java.util.function.Predicate<Object>)candidate->candidate.equals(biome),origin,6400,32,64);
                Map<String,Object> row=new LinkedHashMap<>();row.put("biome",name);
                if(found!=null){
                    List<Integer> location=position(((com.mojang.datafixers.util.Pair<?,?>)found).getFirst());
                    row.put("block_position",location);row.put("chunk",Math.floorDiv(location.get(0),16)+":"+Math.floorDiv(location.get(2),16));
                }
                biomesFound.add(row);
            }
            result.put(dimension,Map.of("structures",structuresFound,"biomes",biomesFound,
                "locators","ChunkGenerator.findNearestMapStructure(level, HolderSet.direct(structure), origin, 100, false) from each search origin, then the start chunk requested at STRUCTURE_STARTS for its complete pieces; ServerLevel.findClosestBiome3d(biome, BlockPos.ZERO, 6400, 32, 64)"));
        }
        return result;
    }
    static Object member(String owner,String name,Object instance)throws Exception {
        return invoke("instanceField",new Class[]{String.class,String.class,Object.class},owner,name,instance);
    }
    static String statusName(Object status)throws Exception {
        return (String)call(MC+"world.level.chunk.status.ChunkStatus","getName",status,new String[]{});
    }
    static Object dependencyMetadata(Object dependencies)throws Exception {
        String owner=MC+"world.level.chunk.status.ChunkDependencies";
        List<Object> radii=new ArrayList<>();
        List<?> nativeList=(List<?>)call(owner,"asList",dependencies,new String[]{});
        for(int radius=0;radius<nativeList.size();++radius)
            radii.add(Map.of("radius",radius,"required_status",statusName(nativeList.get(radius))));
        return Map.of("native_dependency_by_radius",radii,
            "native_radius_by_dependency",member(owner,"radiusByDependency",dependencies),
            "native_radius",call(owner,"getRadius",dependencies,new String[]{}));
    }
    static void observerFailure(Throwable error)throws Exception {
        var stack=new java.io.StringWriter();error.printStackTrace(new java.io.PrintWriter(stack));
        observerFailures.add(stack.toString());
        Files.writeString(taskObserverFailureOutput,stack.toString());
    }
    static synchronized int objectId(Object object) {
        if(object==null)return -1;
        Integer id=observedObjects.get(object);
        if(id==null){
            if(observedObjects.size()>=MAX_OBSERVED_OBJECTS)throw new IllegalStateException("native observer object bound exceeded");
            id=observedObjects.size();observedObjects.put(object,id);
        }
        return id;
    }
    static synchronized int observeCache(Object cache)throws Exception {
        int id=objectId(cache);
        if(!observedCaches.add(cache))return id;
        String owner=MC+"util.StaticCache2D",holder=MC+"server.level.GenerationChunkHolder";
        Map<String,Object> row=new LinkedHashMap<>();
        row.put("cache_object_id",id);
        for(String field:new String[]{"minX","minZ","sizeX","sizeZ"})row.put(field,member(owner,field,cache));
        List<Object> generationTasks=new ArrayList<>();
        Set<Object> seen=Collections.newSetFromMap(new IdentityHashMap<>());
        for(Object entry:(Object[])member(owner,"cache",cache)){
            Object task=((java.util.concurrent.atomic.AtomicReference<?>)member(holder,"task",entry)).get();
            if(task==null || !seen.add(task) || member(MC+"server.level.ChunkGenerationTask","cache",task)!=cache)continue;
            Map<String,Object> taskRow=new LinkedHashMap<>();
            taskRow.put("generation_task_object_id",objectId(task));
            Object pos=member(MC+"server.level.ChunkGenerationTask","pos",task);
            taskRow.put("chunk_x",member(MC+"world.level.ChunkPos","x",pos));
            taskRow.put("chunk_z",member(MC+"world.level.ChunkPos","z",pos));
            taskRow.put("target_status",statusName(member(MC+"server.level.ChunkGenerationTask","targetStatus",task)));
            Object scheduled=member(MC+"server.level.ChunkGenerationTask","scheduledStatus",task);
            taskRow.put("scheduled_status_at_first_cache_observation",scheduled==null?null:statusName(scheduled));
            generationTasks.add(taskRow);
        }
        row.put("native_generation_tasks_sharing_this_cache_at_first_observation",generationTasks);
        nativeStageCaches.add(row);
        return id;
    }
    static List<Object> surfaceBiomeSources(Object cache,Object pos)throws Exception {
        int x=(int)member(MC+"world.level.ChunkPos","x",pos),z=(int)member(MC+"world.level.ChunkPos","z",pos);
        List<Object> sources=new ArrayList<>();
        for(int dz=-1;dz<=1;++dz)for(int dx=-1;dx<=1;++dx){
            Object holder=call(MC+"util.StaticCache2D","get",cache,new String[]{"int","int"},x+dx,z+dz);
            Object source=call(MC+"server.level.GenerationChunkHolder","getLatestChunk",holder,new String[]{});
            Map<String,Object> row=new LinkedHashMap<>();
            row.put("chunk_x",x+dx);row.put("chunk_z",z+dz);row.put("holder_object_id",objectId(holder));
            row.put("chunk_object_id",objectId(source));
            if(source!=null){
                row.put("persisted_status",statusName(call(MC+"world.level.chunk.ChunkAccess","getPersistedStatus",source,new String[]{})));
                synchronized(NativeContextProbe.class){row.put("biomes_completion_invocation",completedBiomeInvocations.get(source));}
                row.put("stored_quart_biomes",NativeContextProbeSnapshot.biomeStorage(source));
            }
            sources.add(row);
        }
        return sources;
    }
    static void taskEvent(String phase,long invocation,int request,Object context,Object step,Object cache,Object chunk,Throwable failure)throws Exception {
        String statuses=MC+"world.level.chunk.status.";
        Object level=call(statuses+"WorldGenContext","level",context,new String[]{});
        Object dimension=call(MC+"world.level.Level","dimension",level,new String[]{});
        Object pos=call(MC+"world.level.chunk.ChunkAccess","getPos",chunk,new String[]{});
        String target=statusName(call(statuses+"ChunkStep","targetStatus",step,new String[]{}));
        Map<String,Object> row=new LinkedHashMap<>();
        row.put("phase",phase);row.put("invocation",invocation);
        row.put(parallelOrigins?"active_parallel_submission_ordinal_hint":"active_synchronous_request_ordinal",request);
        row.put("dimension",call(MC+"resources.ResourceKey","identifier",dimension,new String[]{}).toString());
        row.put("chunk_x",member(MC+"world.level.ChunkPos","x",pos));
        row.put("chunk_z",member(MC+"world.level.ChunkPos","z",pos));
        row.put("chunk_object_id",objectId(chunk));row.put("context_object_id",objectId(context));
        row.put("cache_object_id",observeCache(cache));
        row.put("target_status",target);
        row.put("persisted_status_at_observation",statusName(call(MC+"world.level.chunk.ChunkAccess","getPersistedStatus",chunk,new String[]{})));
        List<String> presentHeightmaps=new ArrayList<>();
        for(Object entry:(Collection<?>)call(MC+"world.level.chunk.ChunkAccess","getHeightmaps",chunk,new String[]{}))
            presentHeightmaps.add(((Enum<?>)((Map.Entry<?,?>)entry).getKey()).name());
        row.put("present_heightmaps_at_observation",presentHeightmaps);
        row.put("thread_name",Thread.currentThread().getName());
        row.put("thread_id",Thread.currentThread().threadId());
        row.put("monotonic_nanos",System.nanoTime());
        if(failure!=null)row.put("failure",failure.toString());
        if(mossTaskProbe!=null && failure==null)
            mossTaskObserve.invoke(mossTaskProbe,phase,invocation,(String)row.get("dimension"),target,cache,chunk);
        if(target.equals("minecraft:surface") && phase.equals("before_native_task"))
            row.put("surface_radius1_stored_biome_sources",surfaceBiomeSources(cache,pos));
        if(target.equals("minecraft:biomes") && phase.equals("native_task_future_completed") && failure==null){
            Map<String,Object> storage=new LinkedHashMap<>();
            storage.put("invocation",invocation);storage.put("chunk_object_id",objectId(chunk));
            storage.put("dimension",row.get("dimension"));storage.put("chunk_x",row.get("chunk_x"));storage.put("chunk_z",row.get("chunk_z"));
            storage.put("stored_quart_biomes",NativeContextProbeSnapshot.biomeStorage(chunk));
            synchronized(NativeContextProbe.class){completedBiomeInvocations.put(chunk,invocation);nativeBiomeStorage.add(storage);}
        }
        synchronized(nativeTaskTrace){
            if(nativeTaskTrace.size()>=MAX_TASK_EVENTS)throw new IllegalStateException("native observer event bound exceeded");
            row.put("event_ordinal",nativeTaskTrace.size());nativeTaskTrace.add(row);
        }
    }
    static void observeNativePyramid(boolean traceTasks)throws Exception {
        String statuses=MC+"world.level.chunk.status.";
        Object pyramid=field(statuses+"ChunkPyramid","GENERATION_PYRAMID");
        List<?> nativeSteps=(List<?>)call(statuses+"ChunkPyramid","steps",pyramid,new String[]{});
        List<Object> metadata=new ArrayList<>();
        List<Object> wrapped=new ArrayList<>();
        Method doWork=(Method)invoke("method",new Class[]{String.class,String.class,String[].class},statuses+"ChunkStatusTask","doWork",
            new String[]{statuses+"WorldGenContext",statuses+"ChunkStep",MC+"util.StaticCache2D",MC+"world.level.chunk.ChunkAccess"});
        Class<?> taskType=(Class<?>)invoke("type",new Class[]{String.class},statuses+"ChunkStatusTask");
        java.util.concurrent.atomic.AtomicLong invocations=new java.util.concurrent.atomic.AtomicLong();
        for(Object step:nativeSteps){
            Object target=call(statuses+"ChunkStep","targetStatus",step,new String[]{});
            Object direct=call(statuses+"ChunkStep","directDependencies",step,new String[]{});
            Object accumulated=call(statuses+"ChunkStep","accumulatedDependencies",step,new String[]{});
            int writeRadius=(int)call(statuses+"ChunkStep","blockStateWriteRadius",step,new String[]{});
            Object task=call(statuses+"ChunkStep","task",step,new String[]{});
            List<String> heightmaps=new ArrayList<>();
            for(Object heightmap:(Set<?>)call(statuses+"ChunkStatus","heightmapsAfter",target,new String[]{}))heightmaps.add(((Enum<?>)heightmap).name());
            metadata.add(Map.of("target_status",statusName(target),"direct_dependencies",dependencyMetadata(direct),
                "accumulated_dependencies",dependencyMetadata(accumulated),"block_state_write_radius",writeRadius,
                "original_native_task_type",task.getClass().getName(),"heightmaps_after",heightmaps));
            if(!traceTasks)continue;
            Object observer=Proxy.newProxyInstance(NativeContextProbe.class.getClassLoader(),new Class<?>[]{taskType},(proxy,method,args)->{
                if(method.getDeclaringClass()==Object.class)return switch(method.getName()){
                    case "hashCode" -> System.identityHashCode(proxy);
                    case "equals" -> proxy==args[0];
                    case "toString" -> "NativeChunkTaskObserver["+task+"]";
                    default -> throw new UnsupportedOperationException(method.getName());
                };
                if(!method.getName().equals("doWork"))throw new UnsupportedOperationException(method.getName());
                long invocation=invocations.getAndIncrement();
                int request=activeNativeRequest;
                try {taskEvent("before_native_task",invocation,request,args[0],step,args[2],args[3],null);}
                catch(Throwable error){observerFailure(error);throw error;}
                Object answer;
                try {answer=doWork.invoke(task,args[0],step,args[2],args[3]);}
                catch(InvocationTargetException error){taskEvent("native_task_threw",invocation,request,args[0],step,args[2],args[3],error.getCause());throw error.getCause();}
                CompletableFuture<?> actualFuture=(CompletableFuture<?>)answer;
                actualFuture.whenComplete((chunk,failure)->{
                    try {taskEvent("native_task_future_completed",invocation,request,args[0],step,args[2],chunk==null?args[3]:chunk,failure);}
                    catch(Throwable error){try{observerFailure(error);}catch(Exception writeFailure){error.addSuppressed(writeFailure);}}
                });
                return actualFuture;
            });
            wrapped.add(make(statuses+"ChunkStep",new String[]{statuses+"ChunkStatus",statuses+"ChunkDependencies",statuses+"ChunkDependencies","int",statuses+"ChunkStatusTask"},
                target,direct,accumulated,writeRadius,observer));
        }
        evidence.put("native_generation_pyramid",metadata);
        if(!traceTasks)return;
        // ChunkStep is a record: retain all native metadata and original task,
        // replacing only list entries with delegating observer records. Do not
        // mutate record final fields or replace native scheduling algorithms.
        Object[] backing=null;
        for(Class<?> owner=nativeSteps.getClass();owner!=null&&backing==null;owner=owner.getSuperclass())for(Field f:owner.getDeclaredFields()){
            if(Modifier.isStatic(f.getModifiers())||f.getType()!=Object[].class)continue;
            f.setAccessible(true);Object[] candidate=(Object[])f.get(nativeSteps);
            if(candidate.length<nativeSteps.size())continue;
            boolean match=true;for(int i=0;i<nativeSteps.size();++i)if(candidate[i]!=nativeSteps.get(i)){match=false;break;}
            if(match){backing=candidate;break;}
        }
        if(backing==null)throw new IllegalStateException("native immutable step list backing storage not observable");
        for(int i=0;i<wrapped.size();++i)backing[i]=wrapped.get(i);
        for(int i=0;i<wrapped.size();++i)if(nativeSteps.get(i)!=wrapped.get(i))throw new IllegalStateException("native step observer not installed");
        evidence.put("native_task_observer_profile","actual ChunkPyramid.GENERATION_PYRAMID step list entries replaced with equivalent native ChunkStep records retaining original target/dependencies/write radius; task proxy delegates original native task with ORIGINAL native step and returns the SAME future; no scheduler, executor, status completion, biome source or cache replacement; completion event observes original task future, and its recorded persisted status reflects actual callback ordering relative to native ChunkStep status commit");
        evidence.put("ordered_native_task_events",nativeTaskTrace);
        evidence.put("native_stage_caches",nativeStageCaches);
        evidence.put("native_biomes_completion_storage",nativeBiomeStorage);
        evidence.put("native_task_observer_bounds",Map.of("max_task_events",MAX_TASK_EVENTS,"max_observed_objects",MAX_OBSERVED_OBJECTS));
        evidence.put("native_task_observer_scope","Ordered task invocation and original-future completion callbacks, actual cache/holder/chunk object identities, complete stored quart biome volumes at BIOMES completion and before SURFACE for its radius-one cache neighborhood. This observes stored source availability, not interception of individual BiomeManager lookups; no native generation call is replaced or repeated.");
    }
    static List<Integer> position(Object pos)throws Exception {
        List<Integer> values=new ArrayList<>();
        for(String axis:new String[]{"getX","getY","getZ"})values.add((int)call(MC+"core.Vec3i",axis,pos,new String[]{}));
        return values;
    }
    static List<Integer> box(Object bounds)throws Exception {
        List<Integer> values=new ArrayList<>();
        for(String axis:new String[]{"minX","minY","minZ","maxX","maxY","maxZ"})values.add((int)call(MC+"world.level.levelgen.structure.BoundingBox",axis,bounds,new String[]{}));
        return values;
    }
    static Object structureProof(Object chunk,Object level,Object server)throws Exception {
        String structure=MC+"world.level.levelgen.structure.";
        Object registry=call(MC+"core.RegistryAccess","lookupOrThrow",call(MC+"server.MinecraftServer","registryAccess",server,new String[]{}),new String[]{MC+"resources.ResourceKey"},field(MC+"core.registries.Registries","STRUCTURE"));
        Object context=call(structure+"pieces.StructurePieceSerializationContext","fromLevel",null,new String[]{MC+"server.level.ServerLevel"},level);
        Map<?,?> starts=(Map<?,?>)call(MC+"world.level.chunk.ChunkAccess","getAllStarts",chunk,new String[]{});
        List<Object> startProofs=new ArrayList<>();
        for(var entry:starts.entrySet()){
            Object start=entry.getValue();
            if(!(boolean)call(structure+"StructureStart","isValid",start,new String[]{}))continue;
            List<Object> pieces=new ArrayList<>();
            for(Object piece:(List<?>)call(structure+"StructureStart","getPieces",start,new String[]{})){
                Map<String,Object> row=new LinkedHashMap<>();
                row.put("native_type",piece.getClass().getName());
                row.put("bounds",box(call(structure+"StructurePiece","getBoundingBox",piece,new String[]{})));
                row.put("native_piece_snbt",call(structure+"StructurePiece","createTag",piece,new String[]{structure+"pieces.StructurePieceSerializationContext"},context).toString());
                if(piece instanceof ffk){
                    Object element=call(structure+"PoolElementStructurePiece","getElement",piece,new String[]{});
                    row.put("template_element",element.toString());
                    row.put("projection",call(structure+"pools.StructurePoolElement","getProjection",element,new String[]{}).toString());
                    row.put("position",position(call(structure+"PoolElementStructurePiece","getPosition",piece,new String[]{})));
                    row.put("ground_level_delta",call(structure+"PoolElementStructurePiece","getGroundLevelDelta",piece,new String[]{}));
                }
                pieces.add(row);
            }
            startProofs.add(Map.of("structure",call(MC+"core.Registry","getKey",registry,new String[]{"java.lang.Object"},entry.getKey()).toString(),
                "start_chunk",call(structure+"StructureStart","getChunkPos",start,new String[]{}).toString(),
                "terrain_adaptation",call(structure+"Structure","terrainAdaptation",entry.getKey(),new String[]{}).toString(),"pieces",pieces));
        }
        List<Object> references=new ArrayList<>();
        for(var entry:((Map<?,?>)call(MC+"world.level.chunk.ChunkAccess","getAllReferences",chunk,new String[]{})).entrySet())
            references.add(Map.of("structure",call(MC+"core.Registry","getKey",registry,new String[]{"java.lang.Object"},entry.getKey()).toString(),"start_chunks",((it.unimi.dsi.fastutil.longs.LongSet)entry.getValue()).toLongArray()));
        Object noise=member(MC+"world.level.chunk.ChunkAccess","noiseChunk",chunk);
        Map<String,Object> beard=new LinkedHashMap<>();
        beard.put("profile","read actual cached NoiseChunk.beardifier used by native pipeline; no synthetic or recomputed beardifier");
        if(noise!=null){
            Object actual=member(MC+"world.level.levelgen.NoiseChunk","beardifier",noise);
            beard.put("native_type",actual.getClass().getName());
            if(actual instanceof eud){
                List<Object> rigid=new ArrayList<>();
                for(Object piece:(List<?>)member(MC+"world.level.levelgen.Beardifier","pieces",actual))
                    rigid.add(Map.of("bounds",box(call(MC+"world.level.levelgen.Beardifier$Rigid","box",piece,new String[]{})),
                        "terrain_adaptation",call(MC+"world.level.levelgen.Beardifier$Rigid","terrainAdjustment",piece,new String[]{}).toString(),
                        "ground_level_delta",call(MC+"world.level.levelgen.Beardifier$Rigid","groundLevelDelta",piece,new String[]{})));
                beard.put("rigid_pieces",rigid);
                List<Object> junctions=new ArrayList<>();
                for(Object junction:(List<?>)member(MC+"world.level.levelgen.Beardifier","junctions",actual)){
                    Map<String,Object> row=new LinkedHashMap<>();
                    for(String accessor:new String[]{"getSourceX","getSourceGroundY","getSourceZ","getDeltaY","getDestProjection"})
                        row.put(accessor,call(structure+"pools.JigsawJunction",accessor,junction,new String[]{}).toString());
                    junctions.add(row);
                }
                beard.put("junctions",junctions);
            }
        }
        return Map.of("starts",startProofs,"references",references,"cached_native_beardifier",beard);
    }
    static Object worldData(anr.a context)throws Exception {
        Object dimensions=call(MC+"world.level.levelgen.presets.WorldPresets","createNormalWorldDimensions",null,new String[]{MC+"core.HolderLookup$Provider"},context.c());
        Object registry=call(MC+"core.RegistryAccess","lookupOrThrow",context.d(),new String[]{MC+"resources.ResourceKey"},field(MC+"core.registries.Registries","LEVEL_STEM"));
        Object complete=call(MC+"world.level.levelgen.WorldDimensions","bake",dimensions,new String[]{MC+"core.Registry"},registry);
        Object rules=make(MC+"world.level.gamerules.GameRules",new String[]{MC+"world.flag.FeatureFlagSet"},context.b().b());
        Object settings=make(MC+"world.level.LevelSettings",new String[]{"java.lang.String",MC+"world.level.GameType","boolean",MC+"world.Difficulty","boolean",MC+"world.level.gamerules.GameRules",MC+"world.level.WorldDataConfiguration"},"native-offline-context",field(MC+"world.level.GameType","SURVIVAL"),false,field(MC+"world.Difficulty","NORMAL"),false,rules,context.b());
        Object options=make(MC+"world.level.levelgen.WorldOptions",new String[]{"long","boolean","boolean"},seed,structures,false);
        Object data=make(MC+"world.level.storage.PrimaryLevelData",new String[]{MC+"world.level.LevelSettings",MC+"world.level.levelgen.WorldOptions",MC+"world.level.storage.PrimaryLevelData$SpecialWorldProperty","com.mojang.serialization.Lifecycle"},settings,options,call(MC+"world.level.levelgen.WorldDimensions$Complete","specialWorldProperty",complete,new String[]{}),call(MC+"world.level.levelgen.WorldDimensions$Complete","lifecycle",complete,new String[]{}));
        Object access=call(MC+"world.level.levelgen.WorldDimensions$Complete","dimensionsRegistryAccess",complete,new String[]{});
        evidence.put("world_data","actual native PrimaryLevelData, normal all3 dimensions, seed="+seed+" structures="+structures+" bonus=false");
        return new anr.b<>((fnm)data,(jr.b)access);
    }
    @SuppressWarnings("removal")
    public static void main(String[] args)throws Exception {
        seed=args.length>2?Long.parseLong(args[2]):0L;
        int coordinateStart=3;
        structures=false;
        if(args.length>coordinateStart && args[coordinateStart].startsWith("--structures=")){
            String value=args[coordinateStart++].substring("--structures=".length());
            if(!value.equals("true") && !value.equals("false"))throw new IllegalArgumentException("--structures requires true or false");
            structures=Boolean.parseBoolean(value);
        }
        boolean traceNativeTasks=args.length>coordinateStart && args[coordinateStart].equals("--trace-native-tasks");
        if(traceNativeTasks)coordinateStart++;
        if(Boolean.getBoolean("native.mossTaskProbe") && !traceNativeTasks)
            throw new IllegalArgumentException("native.mossTaskProbe requires --trace-native-tasks");
        boolean nativeVillageAnchors=args.length>coordinateStart && args[coordinateStart].equals("--native-village-anchors");
        if(nativeVillageAnchors)coordinateStart++;
        if(nativeVillageAnchors && !structures)throw new IllegalArgumentException("--native-village-anchors requires --structures=true");
        boolean extendedStages=args.length>coordinateStart && args[coordinateStart].equals("--extended-stages");
        if(extendedStages)coordinateStart++;
        parallelOrigins=args.length>coordinateStart&&args[coordinateStart].equals("--parallel-origins");
        if(parallelOrigins)coordinateStart++;
        if(parallelOrigins&&!traceNativeTasks)throw new IllegalArgumentException("--parallel-origins requires --trace-native-tasks");
        if(parallelOrigins&&extendedStages)throw new IllegalArgumentException("parallel origins and extended status sweeps require separate native contexts");
        // Discovery runs in its own context: locator queries advance native
        // biome history, so they must never precede a comparable capture.
        boolean discoverAnchors=args.length>coordinateStart&&args[coordinateStart].equals("--discover-anchors");
        if(discoverAnchors)coordinateStart++;
        if(discoverAnchors&&(parallelOrigins||extendedStages||nativeVillageAnchors))throw new IllegalArgumentException("--discover-anchors captures nothing and must run alone");
        Path coordinatesFile=null;
        if(args.length>coordinateStart&&args[coordinateStart].startsWith("--coordinates-file=")){
            coordinatesFile=Path.of(args[coordinateStart++].substring("--coordinates-file=".length()));
            if(parallelOrigins||nativeVillageAnchors||discoverAnchors)throw new IllegalArgumentException("--coordinates-file is only for serial captures");
            if(args.length>coordinateStart)throw new IllegalArgumentException("--coordinates-file replaces positional coordinates");
        }
        List<String> coordinates=new ArrayList<>(args.length>coordinateStart?Arrays.asList(args).subList(coordinateStart,args.length):List.of("0:0"));
        Map<String,List<String>> byDimension=new LinkedHashMap<>();
        if(coordinatesFile!=null){
            Map<String,List<String>> parsed=new Gson().fromJson(Files.readString(coordinatesFile),
                new com.google.gson.reflect.TypeToken<Map<String,List<String>>>(){}.getType());
            for(String dimension:new String[]{"OVERWORLD","NETHER","END"})
                byDimension.put(dimension,new ArrayList<>(parsed.getOrDefault(dimension,List.of())));
            if(!parsed.keySet().stream().allMatch(byDimension::containsKey))throw new IllegalArgumentException("coordinates file has an unknown dimension");
        }else for(String dimension:new String[]{"OVERWORLD","NETHER","END"})byDimension.put(dimension,coordinates);
        for(List<String> list:byDimension.values())for(String coordinate:list)if(!coordinate.matches("-?\\d+:-?\\d+"))throw new IllegalArgumentException("expected chunk-x:chunk-z, got "+coordinate);
        Path root=Path.of(args[0]), output=Path.of(args[1]).toAbsolutePath();
        Files.createDirectories(output.getParent());
        Path work=Files.createTempDirectory(output.getParent(),"offline-world-seed"+seed+"-structures"+structures+"-");
        evidence.clear();
        evidence.put("fresh_world_directory",work.toString());
        evidence.put("structures",structures);
        nativeRequestOrder.clear();
        nativeTaskTrace.clear();activeNativeRequest=-1;
        observedObjects.clear();completedBiomeInvocations.clear();observedCaches.clear();
        nativeStageCaches.clear();nativeBiomeStorage.clear();observerFailures.clear();
        evidence.put("exact_native_origin_request_order",nativeRequestOrder);
        Files.writeString(work.resolve("server.properties"),"online-mode=false\nserver-ip=127.0.0.1\nserver-port=0\nenable-query=false\nenable-rcon=false\nmanagement-server-enabled=false\ntext-filtering-config=\nview-distance=2\nsimulation-distance=2\nmax-tick-time=-1\n");
        taskObserverFailureOutput=output.resolveSibling("native-task-observer-failure.txt");
        var ctor=NativeWorldgenOracle.class.getDeclaredConstructor(Path.class);ctor.setAccessible(true);oracle=ctor.newInstance(root.resolve("server-mappings.txt"));
        mossTaskProbe=null;mossTaskObserve=null;
        if(Boolean.getBoolean("native.mossTaskProbe")){
            Class<?> probe=Class.forName("mossprobe.NativeMossTaskProbe");
            mossTaskProbe=probe.getConstructor(Object.class,Path.class).newInstance(oracle,output.getParent().resolve("moss-task-probe"));
            mossTaskObserve=probe.getMethod("observe",String.class,long.class,String.class,String.class,Object.class,Object.class);
        }
        for(String[] pin:new String[][]{{"server.jar","64bb6d763bed0a9f1d632ec347938594144943ed","SHA-1"},{"server-mappings.txt","5621e9253f05fd57872bbe7f8ddf5f9a7d525955","SHA-1"},{"versions/1.21.11/server-1.21.11.jar","ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc","SHA-256"}})invoke("verify",new Class[]{Path.class,String.class,String.class},root.resolve(pin[0]),pin[1],pin[2]);
        evidence.put("pins_verified",true);
        evidence.put("operation","genuine-native-fullstage");
        evidence.put("invocation",List.of(args));
        evidence.put("seed",seed);
        evidence.put("profile","actual constructor-only native offline DedicatedServer/WorldStem; no game/server entrypoint, initServer, runServer, tick or listener requested");
        final List<String> networkAttempts=Collections.synchronizedList(new ArrayList<>());
        System.setSecurityManager(new SecurityManager(){
            public void checkPermission(java.security.Permission permission){}
            private void deny(String kind){networkAttempts.add(kind+" "+Arrays.toString(Thread.currentThread().getStackTrace()));throw new SecurityException("network forbidden: "+kind);}
            public void checkConnect(String host,int port){deny("connect "+host+":"+port);}
            public void checkListen(int port){deny("listen "+port);}
            public void checkAccept(String host,int port){deny("accept "+host+":"+port);}
        });
        ExecutorService parallelExecutor=null;
        try {
            var loggingFactory=(org.apache.logging.log4j.core.impl.Log4jContextFactory)org.apache.logging.log4j.LogManager.getFactory();
            var loggingContext=loggingFactory.getSelector().getContext("NativeContextProbe",ClassLoader.getSystemClassLoader(),false);
            var logging=new org.apache.logging.log4j.core.config.DefaultConfiguration();
            ((Map<String,String>)logging.getComponent("ContextProperties")).put("hostName","localhost");
            loggingContext.start(logging);
            evidence.put("logging_hostname","explicit native Log4j ContextProperties hostName=localhost; avoids logger DNS without weakening network guard");
            call(MC+"SharedConstants","tryDetectVersion",null,new String[]{});
            call(MC+"server.Bootstrap","bootStrap",null,new String[]{});
            if(Boolean.getBoolean("native.dryRun")){
                // Argument parsing, pins, offline bootstrap and every reflective
                // member the parallel and discovery paths resolve; no server or
                // level is built.
                String level=MC+"server.level.",chunk=MC+"world.level.chunk.status.ChunkStatus";
                String generator=MC+"world.level.chunk.ChunkGenerator",key=MC+"resources.ResourceKey";
                String[][] members={
                    {level+"ServerLevel","getChunkSource"},
                    {level+"ServerChunkCache","getChunkFutureMainThread","int","int",chunk,"boolean"},
                    {level+"ServerChunkCache$MainThreadExecutor","managedBlock","java.util.function.BooleanSupplier"},
                    {level+"ChunkResult","orElse","java.lang.Object"},
                    {level+"ChunkResult","getError"},
                    {MC+"world.level.Level","getChunk","int","int",chunk,"boolean"},
                    {level+"ServerChunkCache","getGenerator"},
                    {MC+"core.HolderGetter","getOrThrow",key},
                    {MC+"core.HolderSet","direct",MC+"core.Holder[]"},
                    {generator,"findNearestMapStructure",level+"ServerLevel",MC+"core.HolderSet",MC+"core.BlockPos","int","boolean"},
                    {generator,"getBiomeSource"},
                    {MC+"world.level.biome.BiomeSource","possibleBiomes"},
                    {MC+"core.Holder","unwrapKey"},
                    {key,"identifier"},
                    {key,"create",key,MC+"resources.Identifier"},
                    {level+"ServerLevel","findClosestBiome3d","java.util.function.Predicate",MC+"core.BlockPos","int","int","int"}};
                List<Object> resolved=new ArrayList<>();
                for(String[] member:members){
                    Method method=(Method)invoke("method",new Class[]{String.class,String.class,String[].class},
                        member[0],member[1],Arrays.copyOfRange(member,2,member.length));
                    resolved.add(Map.of("owner",member[0],"name",member[1],"declaring_class",method.getDeclaringClass().getName(),
                        "declaring_class_public",java.lang.reflect.Modifier.isPublic(method.getDeclaringClass().getModifiers())));
                }
                evidence.put("dry_run",Map.of("coordinates",coordinates,"parallel_origins",parallelOrigins,
                    "trace_native_tasks",traceNativeTasks,"structures",structures,"resolved_members",resolved,
                    "native_context_constructed",false));
                evidence.put("denied_network_attempts",networkAttempts);
                Files.delete(work.resolve("server.properties"));Files.delete(work);evidence.remove("fresh_world_directory");
                Files.writeString(output,new Gson().toJson(evidence));
                System.exit(0);
            }
            observeNativePyramid(traceNativeTasks);
            Files.writeString(output.resolveSibling("native-pyramid-metadata.json"),new Gson().toJson(evidence));
            bak packs=(bak)call(MC+"server.packs.repository.ServerPacksSource","createVanillaTrustedRepository",null,new String[]{});
            Object packConfig=make(MC+"server.WorldLoader$PackConfig",new String[]{MC+"server.packs.repository.PackRepository",MC+"world.level.WorldDataConfiguration","boolean","boolean"},packs,field(MC+"world.level.WorldDataConfiguration","DEFAULT"),false,true);
            Object config=make(MC+"server.WorldLoader$InitConfig",new String[]{MC+"server.WorldLoader$PackConfig",MC+"commands.Commands$CommandSelection",MC+"server.permissions.PermissionSet"},packConfig,field(MC+"commands.Commands$CommandSelection","DEDICATED"),field(MC+"server.permissions.PermissionSet","ALL_PERMISSIONS"));
            Executor direct=Runnable::run;
            anr.f<fnm> supplier=context->{try{return (anr.b<fnm>)worldData(context);}catch(Exception error){throw new CompletionException(error);}};
            anr.e<fnm,ans> factory=ans::new;
            CompletableFuture<?> future=(CompletableFuture<?>)call(MC+"server.WorldLoader","load",null,new String[]{MC+"server.WorldLoader$InitConfig",MC+"server.WorldLoader$WorldDataSupplier",MC+"server.WorldLoader$ResultFactory","java.util.concurrent.Executor","java.util.concurrent.Executor"},config,supplier,factory,direct,direct);
            ans stem=(ans)future.join(); evidence.put("world_stem_constructed",true);
            Object source=call(MC+"world.level.storage.LevelStorageSource","createDefault",null,new String[]{"java.nio.file.Path"},work);
            fni.c storage=(fni.c)call(MC+"world.level.storage.LevelStorageSource","createAccess",source,new String[]{"java.lang.String"},"world");
            asa settings=(asa)make(MC+"server.dedicated.DedicatedServerSettings",new String[]{"java.nio.file.Path"},work.resolve("server.properties"));
            Object fixer=call(MC+"util.datafix.DataFixers","getDataFixer",null,new String[]{});
            ano services=(ano)call(MC+"server.Services","create",null,new String[]{"com.mojang.authlib.yggdrasil.YggdrasilAuthenticationService","java.io.File"},YggdrasilAuthenticationService.createOffline(java.net.Proxy.NO_PROXY),work.toFile());
            ary server=new ary(Thread.currentThread(),storage,packs,stem,settings,(com.mojang.datafixers.DataFixer)fixer,services);
            evidence.put("dedicated_server_constructed",true);
            evidence.put("server_thread",Thread.currentThread().getName());
            evidence.put("server_tick_count",call(MC+"server.MinecraftServer","getTickCount",server,new String[]{}));
            evidence.put("registry_access_type",server.bc().getClass().getName());
            arx players=new arx(server,stem.c(),storage.g());
            server.a(players);
            evidence.put("native_player_list_constructed",true);
            Map<Object,Object> levels=(Map<Object,Object>)invoke("instanceField",new Class[]{String.class,String.class,Object.class},MC+"server.MinecraftServer","levels",server);
            Object stems=call(MC+"core.RegistryAccess","lookupOrThrow",server.bc(),new String[]{MC+"resources.ResourceKey"},field(MC+"core.registries.Registries","LEVEL_STEM"));
            List<Object> levelProofs=new ArrayList<>();
            if(parallelOrigins)parallelExecutor=Executors.newFixedThreadPool(2,
                Thread.ofPlatform().name("native-reference-generation-",0).factory());
            Executor generationExecutor=parallelExecutor==null?direct:parallelExecutor;
            for(String dimension:new String[]{"OVERWORLD","NETHER","END"}){
                Object stemKey=field(MC+"world.level.dimension.LevelStem",dimension);
                Object nativeStem=call(MC+"core.Registry","getValueOrThrow",stems,new String[]{MC+"resources.ResourceKey"},stemKey);
                Object levelKey=field(MC+"world.level.Level",dimension.equals("NETHER")?"NETHER":dimension);
                Object levelData=dimension.equals("OVERWORLD")?stem.d():make(MC+"world.level.storage.DerivedLevelData",new String[]{MC+"world.level.storage.WorldData",MC+"world.level.storage.ServerLevelData"},stem.d(),stem.d());
                Object sequences=make(MC+"world.RandomSequences",new String[]{});
                long zoomSeed=(long)call(MC+"world.level.biome.BiomeManager","obfuscateSeed",null,new String[]{"long"},seed);
                Object level=make(MC+"server.level.ServerLevel",new String[]{MC+"server.MinecraftServer","java.util.concurrent.Executor",MC+"world.level.storage.LevelStorageSource$LevelStorageAccess",MC+"world.level.storage.ServerLevelData",MC+"resources.ResourceKey",MC+"world.level.dimension.LevelStem","boolean","long","java.util.List","boolean",MC+"world.RandomSequences"},server,generationExecutor,storage,levelData,levelKey,nativeStem,false,zoomSeed,List.of(),false,sequences);
                levels.put(levelKey,level);
                levelProofs.add(Map.of("dimension",dimension,"native_type",level.getClass().getName(),"biome_zoom_seed",zoomSeed,
                    "world_seed",call(MC+"server.level.ServerLevel","getSeed",level,new String[]{}),
                    "structure_manager_type",call(MC+"server.level.ServerLevel","structureManager",level,new String[]{}).getClass().getName()));
                Object manager=call(MC+"server.level.ServerLevel","structureManager",level,new String[]{});
                boolean actualStructures=(boolean)call(MC+"world.level.StructureManager","shouldGenerateStructures",manager,new String[]{});
                if(actualStructures!=structures)throw new IllegalStateException("native StructureManager flag differs from requested WorldOptions");
                evidence.put("native_structure_flag_"+dimension,actualStructures);
            }
            evidence.put("native_levels_constructed",levelProofs);
            if(nativeVillageAnchors){
                Object overworld=levels.get(field(MC+"world.level.Level","OVERWORLD"));
                Object origin=make(MC+"core.BlockPos",new String[]{"int","int","int"},0,0,0);
                Object anchor=call(MC+"server.level.ServerLevel","findNearestMapStructure",overworld,new String[]{MC+"tags.TagKey",MC+"core.BlockPos","int","boolean"},field(MC+"tags.StructureTags","VILLAGE"),origin,16,false);
                if(anchor==null)throw new IllegalStateException("native village locator found no start within radius 16");
                List<Integer> location=position(anchor);
                int anchorX=Math.floorDiv(location.get(0),16),anchorZ=Math.floorDiv(location.get(2),16);
                for(int dz=-1;dz<=1;dz++)for(int dx=-1;dx<=1;dx++){
                    String coordinate=(anchorX+dx)+":"+(anchorZ+dz);
                    if(!coordinates.contains(coordinate))coordinates.add(coordinate);
                }
                evidence.put("native_village_anchor",Map.of("native_method","ServerLevel.findNearestMapStructure(StructureTags.VILLAGE, BlockPos.ZERO,16,false)","block_position",location,"capture_selection","native returned start plus surrounding 3x3 chunks; no coordinate algorithm specialcases"));
            }
            Map<String,Object> structureProofs=new LinkedHashMap<>();
            if(structures)evidence.put("native_structure_provenance",structureProofs);
            boolean repeatedCoordinates=byDimension.values().stream().anyMatch(list->new HashSet<>(list).size()!=list.size());
            List<String> captures=new ArrayList<>();
            evidence.put("coordinate_request_order",coordinatesFile==null?coordinates:byDimension);
            evidence.put("query_profile","all requested SURFACE captures first, then CARVERS, then FEATURES; for each stage dimensions OW/Nether/End then that dimension's coordinates in argument order; each snapshot immediately after its requested native status future resolves, before any subsequent target request; native dependency scheduling unchanged; snapshots are stage completion observations, not final neighbor-decoration closure");
            if(discoverAnchors){
                evidence.put("query_profile","native anchor discovery only: structure and biome locators, no chunk captures");
                evidence.put("native_anchors",discoverAnchors(levels,server));
            }else if(parallelOrigins){
                evidence.put("query_profile","native asynchronous origin batches; submit all target futures before managedBlock; observe all targets only after the whole status batch completes; original native dependency scheduling and worker histories retained; no FEATURES halo closure, higher status or ticks");
                evidence.put("parallel_generation_runtime",Map.of(
                    "available_processors",Runtime.getRuntime().availableProcessors(),
                    "requested_max_background_threads",System.getProperty("max.bg.threads","native default"),
                    "original_native_task_events","ordered_native_task_events"));
                activeCoordinateOrder=coordinates;
                for(String stage:new String[]{"SURFACE","CARVERS","FEATURES"})
                    for(String dimension:new String[]{"OVERWORLD","NETHER","END"})
                        parallelStage(levels.get(field(MC+"world.level.Level",dimension)),server,dimension,stage,
                            coordinates,captures,output,structureProofs);
            }else{
            if(extendedStages){
                evidence.put("extended_stage_request_order",List.of("EMPTY","STRUCTURE_STARTS","STRUCTURE_REFERENCES","BIOMES","NOISE","SURFACE","CARVERS","FEATURES","FEATURES_HALO_CLOSURE","INITIALIZE_LIGHT","LIGHT","SPAWN","FULL"));
                for(String stage:new String[]{"EMPTY","STRUCTURE_STARTS","STRUCTURE_REFERENCES","BIOMES","NOISE"})for(String dimension:new String[]{"OVERWORLD","NETHER","END"}){
                    List<String> dimensionCoordinates=byDimension.get(dimension);activeCoordinateOrder=dimensionCoordinates;
                    for(int coordinateIndex=0;coordinateIndex<dimensionCoordinates.size();++coordinateIndex){
                        String coordinate=dimensionCoordinates.get(coordinateIndex);activeCoordinateRequestIndex=coordinateIndex;
                        String[] parts=coordinate.split(":");int x=Integer.parseInt(parts[0]),z=Integer.parseInt(parts[1]);
                        Object level=levels.get(field(MC+"world.level.Level",dimension));
                        Object chunk=requestChunk(level,dimension,x,z,stage);
                        String suffix=repeatedCoordinates?"-request"+(nativeRequestOrder.size()-1):"";
                        Path capture=output.getParent().resolve("extended-"+dimension.toLowerCase(Locale.ROOT)+"-"+stage.toLowerCase(Locale.ROOT)+"-seed"+seed+"-chunk"+x+"_"+z+suffix+".json");
                        NativeContextProbeSnapshot.snapshot(chunk,server,dimension,stage,capture);captures.add(capture.toString());
                    }
                }
            }
            for(String stage:new String[]{"SURFACE","CARVERS","FEATURES"})for(String dimension:new String[]{"OVERWORLD","NETHER","END"}){
                List<String> dimensionCoordinates=byDimension.get(dimension);activeCoordinateOrder=dimensionCoordinates;
                for(int coordinateIndex=0;coordinateIndex<dimensionCoordinates.size();++coordinateIndex){
                    String coordinate=dimensionCoordinates.get(coordinateIndex);activeCoordinateRequestIndex=coordinateIndex;
                    String[] parts=coordinate.split(":");int x=Integer.parseInt(parts[0]),z=Integer.parseInt(parts[1]);
                    Object level=levels.get(field(MC+"world.level.Level",dimension));
                    Object chunk=requestChunk(level,dimension,x,z,stage);
                    String suffix=repeatedCoordinates?"-request"+(nativeRequestOrder.size()-1):"";
                    Path capture=output.getParent().resolve("fullstage-"+dimension.toLowerCase(Locale.ROOT)+"-"+stage.toLowerCase(Locale.ROOT)+"-seed"+seed+"-chunk"+x+"_"+z+suffix+".json");
                    NativeContextProbeSnapshot.snapshot(chunk,server,dimension,stage,capture);captures.add(capture.toString());
                    if(structures)structureProofs.put(capture.getFileName().toString(),structureProof(chunk,level,server));
                }
            }
            Object features=field(MC+"world.level.chunk.status.ChunkStatus","FEATURES");
            Object featureStep=call(MC+"world.level.chunk.status.ChunkPyramid","getStepTo",
                field(MC+"world.level.chunk.status.ChunkPyramid","GENERATION_PYRAMID"),
                new String[]{MC+"world.level.chunk.status.ChunkStatus"},features);
            int writeRadius=(int)call(MC+"world.level.chunk.status.ChunkStep","blockStateWriteRadius",featureStep,new String[]{});
            List<String> haloOrder=new ArrayList<>();
            for(String dimension:new String[]{"OVERWORLD","NETHER","END"}){
                Object level=levels.get(field(MC+"world.level.Level",dimension));
                List<String> dimensionCoordinates=byDimension.get(dimension);
                Set<String> requested=new LinkedHashSet<>(dimensionCoordinates);
                for(String coordinate:dimensionCoordinates){
                    String[] parts=coordinate.split(":");int x=Integer.parseInt(parts[0]),z=Integer.parseInt(parts[1]);
                    for(int dz=-writeRadius;dz<=writeRadius;++dz)for(int dx=-writeRadius;dx<=writeRadius;++dx){
                        String neighbor=(x+dx)+":"+(z+dz);
                        if(!requested.add(neighbor))continue;
                        requestChunk(level,dimension,x+dx,z+dz,"FEATURES");
                        haloOrder.add(dimension+":"+neighbor);
                    }
                }
            }
            evidence.put("features_halo",Map.of("native_features_block_state_write_radius",writeRadius,"additional_native_features_request_order",haloOrder,
                "profile","all target FEATURES then all possible native FEATURES writers within native blockStateWriteRadius, no higher stage; halo requests z-major/x-minor per argument-order target, deduplicated per dimension; snapshot targets only after all halo requests"));
            for(String dimension:new String[]{"OVERWORLD","NETHER","END"}){
                List<String> dimensionCoordinates=byDimension.get(dimension);activeCoordinateOrder=dimensionCoordinates;
                for(int coordinateIndex=0;coordinateIndex<dimensionCoordinates.size();++coordinateIndex){
                    String coordinate=dimensionCoordinates.get(coordinateIndex);activeCoordinateRequestIndex=coordinateIndex;
                    String[] parts=coordinate.split(":");int x=Integer.parseInt(parts[0]),z=Integer.parseInt(parts[1]);
                    Object level=levels.get(field(MC+"world.level.Level",dimension));
                    Object chunk=requestChunk(level,dimension,x,z,"FEATURES");
                    String suffix=repeatedCoordinates?"-request"+(nativeRequestOrder.size()-1):"";
                    Path capture=output.getParent().resolve("fullstage-"+dimension.toLowerCase(Locale.ROOT)+"-features-halo-seed"+seed+"-chunk"+x+"_"+z+suffix+".json");
                    NativeContextProbeSnapshot.snapshot(chunk,server,dimension,"FEATURES_HALO_CLOSURE",capture);captures.add(capture.toString());
                    if(structures)structureProofs.put(capture.getFileName().toString(),structureProof(chunk,level,server));
                }
            }
            if(extendedStages)for(String stage:new String[]{"INITIALIZE_LIGHT","LIGHT","SPAWN","FULL"})for(String dimension:new String[]{"OVERWORLD","NETHER","END"}){
                List<String> dimensionCoordinates=byDimension.get(dimension);activeCoordinateOrder=dimensionCoordinates;
                for(int coordinateIndex=0;coordinateIndex<dimensionCoordinates.size();++coordinateIndex){
                    String coordinate=dimensionCoordinates.get(coordinateIndex);activeCoordinateRequestIndex=coordinateIndex;
                    String[] parts=coordinate.split(":");int x=Integer.parseInt(parts[0]),z=Integer.parseInt(parts[1]);
                    Object level=levels.get(field(MC+"world.level.Level",dimension));
                    Object chunk=requestChunk(level,dimension,x,z,stage);
                    String suffix=repeatedCoordinates?"-request"+(nativeRequestOrder.size()-1):"";
                    Path capture=output.getParent().resolve("extended-"+dimension.toLowerCase(Locale.ROOT)+"-"+stage.toLowerCase(Locale.ROOT)+"-seed"+seed+"-chunk"+x+"_"+z+suffix+".json");
                    NativeContextProbeSnapshot.snapshot(chunk,server,dimension,stage,capture);captures.add(capture.toString());
                }
            }
            }
            evidence.put("captures",captures);
            evidence.put("server_tick_count_after_chunk_requests",call(MC+"server.MinecraftServer","getTickCount",server,new String[]{}));
        }catch(Throwable error){
            var stack=new java.io.StringWriter();error.printStackTrace(new java.io.PrintWriter(stack));evidence.put("failure",stack.toString());
        }finally{
            if(parallelExecutor!=null){
                parallelExecutor.shutdown();
                if(!parallelExecutor.awaitTermination(30,TimeUnit.SECONDS)){
                    parallelExecutor.shutdownNow();
                    evidence.putIfAbsent("failure","native generation executor did not quiesce before cleanup deadline");
                }
            }
            evidence.put("denied_network_attempts",networkAttempts);
            if(!observerFailures.isEmpty())evidence.put("observer_failures",new ArrayList<>(observerFailures));
            if(traceNativeTasks)synchronized(nativeTaskTrace){evidence.put("ordered_native_task_events",new ArrayList<>(nativeTaskTrace));}
            Files.writeString(output,new Gson().toJson(evidence));
        }
        System.exit(evidence.containsKey("failure")||!observerFailures.isEmpty()?1:0);
    }
}
