/* Controlled Java 1.21.11 WorldGenRegion/ProtoChunk actor observations.
 * Constructor-only native server context; no server loop, listeners or ticks.
 * Run with -XX:ActiveProcessorCount=1 -Xmx1G and the pinned bundled classpath.
 */
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.*;
import java.lang.reflect.*;
import java.io.*;
import java.util.zip.*;
import com.mojang.authlib.yggdrasil.YggdrasilAuthenticationService;

public final class NativePendingActorProbe {
    static final String MC=NativeContextProbe.MC;
    static final Map<String,Object> evidence=NativeContextProbe.evidence;
    static Object invoke(String name,Class<?>[] types,Object... args)throws Exception{return NativeContextProbe.invoke(name,types,args);}
    static Object call(String owner,String name,Object receiver,String[] types,Object... args)throws Exception{return NativeContextProbe.call(owner,name,receiver,types,args);}
    static Object field(String owner,String name)throws Exception{return NativeContextProbe.field(owner,name);}
    static Object member(String owner,String name,Object instance)throws Exception{return NativeContextProbe.member(owner,name,instance);}
    static Object make(String owner,String[] types,Object... args)throws Exception{return NativeContextProbe.make(owner,types,args);}
    static Object requestChunk(Object level,String dimension,int x,int z,String status)throws Exception{return NativeContextProbe.requestChunk(level,dimension,x,z,status);}
    static Object worldData(anr.a context)throws Exception{return NativeContextProbe.worldData(context);}
    static final String CH=MC+"world.level.chunk.", POS=MC+"core.BlockPos", STATE=MC+"world.level.block.state.BlockState";
    static final List<Object> observations=new ArrayList<>();
    static final List<String> deniedNetwork=Collections.synchronizedList(new ArrayList<>());
    static Object region,chunk,position,server;
    static boolean fullBoundary;
    static Object state(String name)throws Exception {
        Object registry=field(MC+"core.registries.BuiltInRegistries","BLOCK");
        Object id=call(MC+"resources.Identifier","parse",null,new String[]{"java.lang.String"},name);
        return call(MC+"world.level.block.Block","defaultBlockState",call(MC+"core.Registry","getValue",registry,new String[]{MC+"resources.Identifier"},id),new String[]{});
    }
    static void observe(String operation)throws Exception {
        Object saved=call(CH+"ChunkAccess","getBlockEntityNbtForSaving",chunk,new String[]{POS,MC+"core.HolderLookup$Provider"},position,((ary)server).bc());
        Map<String,Object> row=new LinkedHashMap<>();row.put("operation",operation);
        row.put("state",invoke("canonicalState",new Class[]{Object.class},call(CH+"ProtoChunk","getBlockState",chunk,new String[]{POS},position)));
        row.put("saved",saved==null?null:NativeContextProbeSnapshot.nbt(saved));
        row.put("live",((Map<?,?>)call(CH+"ProtoChunk","getBlockEntities",chunk,new String[]{})).size());
        row.put("pending",((Map<?,?>)call(CH+"ProtoChunk","getBlockEntityNbts",chunk,new String[]{})).size());
        observations.add(row);
    }
    static void place(String label,Object block)throws Exception {
        boolean result=(boolean)call(MC+"server.level.WorldGenRegion","setBlock",region,new String[]{POS,STATE,"int","int"},position,block,2,512);
        observe(label+":"+result);
    }
    static void capture(Path runtime,ary nativeServer,Object level)throws Exception {
        server=nativeServer;
        chunk=requestChunk(level,"OVERWORLD",0,0,"CARVERS");
        Object source=call(MC+"server.level.ServerLevel","getChunkSource",level,new String[]{});
        Object map=member(MC+"server.level.ServerChunkCache","chunkMap",source);
        Object step=call(CH+"status.ChunkPyramid","getStepTo",field(CH+"status.ChunkPyramid","GENERATION_PYRAMID"),new String[]{CH+"status.ChunkStatus"},field(CH+"status.ChunkStatus","FEATURES"));
        Map<String,Object> holders=new HashMap<>();
        for(int z=-1;z<=1;z++)for(int x=-1;x<=1;x++) {
            requestChunk(level,"OVERWORLD",x,z,"CARVERS");
            long key=(long)x&0xffffffffL|((long)z&0xffffffffL)<<32;
            holders.put(x+":"+z,call(MC+"server.level.ChunkMap","acquireGeneration",map,new String[]{"long"},key));
        }
        Class<?> initializer=Class.forName("bhe$a");
        Object init=Proxy.newProxyInstance(initializer.getClassLoader(),new Class[]{initializer},(p,m,a)->holders.get(a[0]+":"+a[1]));
        Object cache=call(MC+"util.StaticCache2D","create",null,new String[]{"int","int","int",MC+"util.StaticCache2D$Initializer"},0,0,1,init);
        region=make(MC+"server.level.WorldGenRegion",new String[]{MC+"server.level.ServerLevel",MC+"util.StaticCache2D",CH+"status.ChunkStep",CH+"ChunkAccess"},level,cache,step,chunk);
        position=make(POS,new String[]{"int","int","int"},1,200,1);
        Object chest=state("minecraft:chest"),barrel=state("minecraft:barrel"),stone=state("minecraft:stone");
        place("new_chest",chest);
        place("same_chest",chest);
        Object waterlogged=field(MC+"world.level.block.state.properties.BlockStateProperties","WATERLOGGED");
        Object wet=call(MC+"world.level.block.state.StateHolder","setValue",chest,new String[]{MC+"world.level.block.state.properties.Property","java.lang.Comparable"},waterlogged,true);
        place("wet_chest",wet);
        place("pending_chest_to_barrel",barrel);
        place("pending_to_stone",stone);
        place("stone_to_chest",chest);
        Object live=call(MC+"server.level.WorldGenRegion","getBlockEntity",region,new String[]{POS},position);
        Object table=call(MC+"resources.ResourceKey","create",null,new String[]{MC+"resources.ResourceKey",MC+"resources.Identifier"},field(MC+"core.registries.Registries","LOOT_TABLE"),call(MC+"resources.Identifier","parse",null,new String[]{"java.lang.String"},"minecraft:chests/simple_dungeon"));
        call(MC+"world.RandomizableContainer","setLootTable",live,new String[]{MC+"resources.ResourceKey","long"},table,Long.MIN_VALUE);
        observe("explicit_loot");
        place("live_chest_property_change",wet);
        place("live_chest_to_barrel",barrel);
        call(MC+"world.RandomizableContainer","setLootTable",call(MC+"server.level.WorldGenRegion","getBlockEntity",region,new String[]{POS},position),new String[]{MC+"resources.ResourceKey","long"},table,0L);
        observe("explicit_loot_after_actor_replacement");
        place("live_to_stone",stone);
        place("stone_to_barrel",barrel);
        Object template=make(MC+"nbt.CompoundTag",new String[]{});
        call(MC+"nbt.CompoundTag","putString",template,new String[]{"java.lang.String","java.lang.String"},"id","minecraft:barrel");
        call(MC+"nbt.CompoundTag","putString",template,new String[]{"java.lang.String","java.lang.String"},"LootTable","minecraft:chests/shipwreck_supply");
        call(MC+"nbt.CompoundTag","putLong",template,new String[]{"java.lang.String","long"},"LootTableSeed",17L);
        Object actor=call(MC+"world.level.block.entity.BlockEntity","loadStatic",null,new String[]{POS,STATE,MC+"nbt.CompoundTag",MC+"core.HolderLookup$Provider"},position,barrel,template,nativeServer.bc());
        call(CH+"ProtoChunk","setBlockEntity",chunk,new String[]{MC+"world.level.block.entity.BlockEntity"},actor);
        observe("explicit_template");
        place("template_barrel_same_state",barrel);
        call(MC+"nbt.CompoundTag","putLong",template,new String[]{"java.lang.String","long"},"LootTableSeed",-29L);
        Object replacement=call(MC+"world.level.block.entity.BlockEntity","loadStatic",null,new String[]{POS,STATE,MC+"nbt.CompoundTag",MC+"core.HolderLookup$Provider"},position,barrel,template,nativeServer.bc());
        call(CH+"ProtoChunk","setBlockEntity",chunk,new String[]{MC+"world.level.block.entity.BlockEntity"},replacement);
        observe("explicit_template_over_live");
        place("template_to_air",state("minecraft:air"));
        position=make(POS,new String[]{"int","int","int"},1,320,1);
        place("above_bounds_chest",chest);
        List<String> actorBlocks=new ArrayList<>();
        Map<String,String> actorIds=new TreeMap<>();
        for(Object block:(Iterable<?>)field(MC+"core.registries.BuiltInRegistries","BLOCK")) {
            Object blockState=call(MC+"world.level.block.Block","defaultBlockState",block,new String[]{});
            if(!(boolean)call(MC+"world.level.block.state.BlockBehaviour$BlockStateBase","hasBlockEntity",blockState,new String[]{}))continue;
            String blockId=call(MC+"core.Registry","getKey",field(MC+"core.registries.BuiltInRegistries","BLOCK"),new String[]{"java.lang.Object"},block).toString();
            actorBlocks.add(blockId);
            Object fresh=call(MC+"world.level.block.EntityBlock","newBlockEntity",block,new String[]{POS,STATE},position,blockState);
            if(fresh==null){actorIds.put(blockId,null);continue;}
            Object type=call(MC+"world.level.block.entity.BlockEntity","getType",fresh,new String[]{});
            actorIds.put(blockId,call(MC+"core.Registry","getKey",field(MC+"core.registries.BuiltInRegistries","BLOCK_ENTITY_TYPE"),new String[]{"java.lang.Object"},type).toString());
        }
        Collections.sort(actorBlocks);evidence.put("actor_blocks",actorBlocks);evidence.put("actor_ids",actorIds);
        evidence.put("transitions",observations);
        evidence.put("native_tick_count",call(MC+"server.MinecraftServer","getTickCount",nativeServer,new String[]{}));
        captureTemplates(runtime,nativeServer);
        if(fullBoundary)captureFullBoundary(level,nativeServer,actorBlocks);
    }
    static Object get(Object tag,String name)throws Exception {
        return call(MC+"nbt.CompoundTag","get",tag,new String[]{"java.lang.String"},name);
    }
    static String text(Object tag,String name)throws Exception {
        return (String)call(MC+"nbt.CompoundTag","getStringOr",tag,new String[]{"java.lang.String","java.lang.String"},name,"");
    }
    static byte[] bytes(Object tag)throws Exception {
        ByteArrayOutputStream output=new ByteArrayOutputStream();
        call(MC+"nbt.NbtIo","write",null,new String[]{MC+"nbt.CompoundTag","java.io.DataOutput"},tag,new DataOutputStream(output));
        return output.toByteArray();
    }
    static void captureTemplates(Path runtime,ary nativeServer)throws Exception {
        List<Object> records=new ArrayList<>();
        Object zero=make(POS,new String[]{"int","int","int"},0,0,0);
        Object lookup=call(MC+"core.HolderLookup$Provider","lookupOrThrow",nativeServer.bc(),new String[]{MC+"resources.ResourceKey"},field(MC+"core.registries.Registries","BLOCK"));
        try(ZipFile jar=new ZipFile(runtime.resolve("versions/1.21.11/server-1.21.11.jar").toFile())) {
            List<? extends ZipEntry> entries=Collections.list(jar.entries()).stream().filter(e->e.getName().startsWith("data/minecraft/structure/")&&e.getName().endsWith(".nbt")).sorted(Comparator.comparing(ZipEntry::getName)).toList();
            for(ZipEntry entry:entries) {
                Object root;
                try(InputStream input=jar.getInputStream(entry)) {
                    root=call(MC+"nbt.NbtIo","readCompressed",null,new String[]{"java.io.InputStream",MC+"nbt.NbtAccounter"},input,call(MC+"nbt.NbtAccounter","unlimitedHeap",null,new String[]{}));
                }
                List<?> blocks=(List<?>)get(root,"blocks");
                Object p=get(root,"palette");
                List<?> palette=p==null?(List<?>)((List<?>)get(root,"palettes")).getFirst():(List<?>)p;
                for(int index=0;index<blocks.size();index++) {
                    Object block=blocks.get(index),source=get(block,"nbt");if(source==null)continue;
                    String id=text(source,"id");
                    if(id.equals("minecraft:jigsaw")||(id.equals("minecraft:structure_block")&&text(source,"mode").equals("DATA")))continue;
                    int stateIndex=(int)call(MC+"nbt.CompoundTag","getIntOr",block,new String[]{"java.lang.String","int"},"state",-1);
                    Object state=call(MC+"nbt.NbtUtils","readBlockState",null,new String[]{MC+"core.HolderGetter",MC+"nbt.CompoundTag"},lookup,palette.get(stateIndex));
                    Object actor=call(MC+"world.level.block.entity.BlockEntity","loadStatic",null,new String[]{POS,STATE,MC+"nbt.CompoundTag",MC+"core.HolderLookup$Provider"},zero,state,source,nativeServer.bc());
                    if(actor==null)throw new IllegalStateException("native loadStatic rejected "+entry.getName()+"#"+index);
                    call(CH+"ProtoChunk","setBlockState",chunk,new String[]{POS,STATE,"int"},zero,state,2);
                    call(CH+"ProtoChunk","setBlockEntity",chunk,new String[]{MC+"world.level.block.entity.BlockEntity"},actor);
                    Object saved=call(CH+"ProtoChunk","getBlockEntityNbtForSaving",chunk,new String[]{POS,MC+"core.HolderLookup$Provider"},zero,nativeServer.bc());
                    byte[] raw=bytes(source);List<Integer> unsigned=new ArrayList<>(raw.length);
                    for(byte value:raw)unsigned.add(value&255);
                    records.add(Map.of("template",entry.getName(),"index",index,"source_nbt",Base64.getEncoder().encodeToString(raw),"source_bytes",unsigned,"saved",NativeContextProbeSnapshot.nbt(saved)));
                }
            }
        }
        evidence.put("proto_template_actors",records);
        evidence.put("proto_template_actor_count",records.size());
    }
    static void captureFullBoundary(Object level,ary nativeServer,List<String> actorBlocks)throws Exception {
        // Quiesce every possible FEATURES writer before injecting the controlled
        // actor grid. Otherwise its artificial roof would change later features.
        for(int z=-1;z<=1;z++)for(int x=-1;x<=1;x++)requestChunk(level,"OVERWORLD",x,z,"FEATURES");
        if(actorBlocks.size()>256)throw new IllegalStateException("actor grid exceeds one chunk");
        List<String> blocks=new ArrayList<>(actorBlocks);
        List<int[]> positions=new ArrayList<>();
        for(int i=0;i<blocks.size();i++)positions.add(new int[]{i&15,200,i>>4});
        blocks.add("minecraft:chest");positions.add(new int[]{4,400,1});
        List<Object> rows=new ArrayList<>();
        for(int i=0;i<positions.size();i++) {
            int[] xyz=positions.get(i);position=make(POS,new String[]{"int","int","int"},xyz[0],xyz[1],xyz[2]);
            boolean placed=(boolean)call(MC+"server.level.WorldGenRegion","setBlock",region,new String[]{POS,STATE,"int","int"},position,state(blocks.get(i)),2,512);
            Object saved=call(CH+"ChunkAccess","getBlockEntityNbtForSaving",chunk,new String[]{POS,MC+"core.HolderLookup$Provider"},position,nativeServer.bc());
            Map<String,Object> row=new LinkedHashMap<>();row.put("position",xyz);row.put("block",blocks.get(i));row.put("set_result",placed);
            row.put("proto_state",invoke("canonicalState",new Class[]{Object.class},call(MC+"world.level.BlockGetter","getBlockState",chunk,new String[]{POS},position)));
            row.put("proto_saved",saved==null?null:NativeContextProbeSnapshot.nbt(saved));rows.add(row);
        }
        Object full=requestChunk(level,"OVERWORLD",0,0,"FULL");
        evidence.put("full_chunk_class",full.getClass().getName());
        for(int i=0;i<positions.size();i++) {
            int[] xyz=positions.get(i);position=make(POS,new String[]{"int","int","int"},xyz[0],xyz[1],xyz[2]);
            Object saved=call(CH+"ChunkAccess","getBlockEntityNbtForSaving",full,new String[]{POS,MC+"core.HolderLookup$Provider"},position,nativeServer.bc());
            Map<String,Object> row=(Map<String,Object>)rows.get(i);
            row.put("full_saved",saved==null?null:NativeContextProbeSnapshot.nbt(saved));
            row.put("full_state",invoke("canonicalState",new Class[]{Object.class},call(MC+"world.level.BlockGetter","getBlockState",full,new String[]{POS},position)));
        }
        evidence.put("full_boundary_actors",rows);
        evidence.put("full_boundary_registered_actor_count",actorBlocks.size());
        evidence.put("native_tick_count",call(MC+"server.MinecraftServer","getTickCount",nativeServer,new String[]{}));
    }
    @SuppressWarnings("removal")
    public static void main(String[] args)throws Exception {
        Path root=Path.of(args[0]),output=Path.of(args[1]);NativeContextProbe.seed=0;NativeContextProbe.structures=false;
        fullBoundary=args.length>2&&args[2].equals("--full-boundary");
        var ctor=NativeWorldgenOracle.class.getDeclaredConstructor(Path.class);ctor.setAccessible(true);NativeContextProbe.oracle=ctor.newInstance(root.resolve("server-mappings.txt"));
        for(String[] pin:new String[][]{{"server.jar","64bb6d763bed0a9f1d632ec347938594144943ed","SHA-1"},{"server-mappings.txt","5621e9253f05fd57872bbe7f8ddf5f9a7d525955","SHA-1"},{"versions/1.21.11/server-1.21.11.jar","ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc","SHA-256"}})invoke("verify",new Class[]{Path.class,String.class,String.class},root.resolve(pin[0]),pin[1],pin[2]);
        System.setSecurityManager(new SecurityManager(){
            public void checkPermission(java.security.Permission p){}
            private void deny(String operation){deniedNetwork.add(operation);throw new SecurityException("network forbidden: "+operation);}
            public void checkConnect(String h,int p){deny("connect "+h+":"+p);}
            public void checkListen(int p){deny("listen "+p);}
            public void checkAccept(String h,int p){deny("accept "+h+":"+p);}
        });
        try {
            var ctx=((org.apache.logging.log4j.core.impl.Log4jContextFactory)org.apache.logging.log4j.LogManager.getFactory()).getSelector().getContext("NativePendingActorProbe",ClassLoader.getSystemClassLoader(),false);
            var logging=new org.apache.logging.log4j.core.config.DefaultConfiguration();((Map<String,String>)logging.getComponent("ContextProperties")).put("hostName","localhost");ctx.start(logging);
            call(MC+"SharedConstants","tryDetectVersion",null,new String[]{});call(MC+"server.Bootstrap","bootStrap",null,new String[]{});
            bak packs=(bak)call(MC+"server.packs.repository.ServerPacksSource","createVanillaTrustedRepository",null,new String[]{});
            Object packConfig=make(MC+"server.WorldLoader$PackConfig",new String[]{MC+"server.packs.repository.PackRepository",MC+"world.level.WorldDataConfiguration","boolean","boolean"},packs,field(MC+"world.level.WorldDataConfiguration","DEFAULT"),false,true);
            Object config=make(MC+"server.WorldLoader$InitConfig",new String[]{MC+"server.WorldLoader$PackConfig",MC+"commands.Commands$CommandSelection",MC+"server.permissions.PermissionSet"},packConfig,field(MC+"commands.Commands$CommandSelection","DEDICATED"),field(MC+"server.permissions.PermissionSet","ALL_PERMISSIONS"));
            Executor direct=Runnable::run;anr.f<fnm> supplier=c->{try{return (anr.b<fnm>)worldData(c);}catch(Exception e){throw new CompletionException(e);}};anr.e<fnm,ans> factory=ans::new;
            ans stem=(ans)((CompletableFuture<?>)call(MC+"server.WorldLoader","load",null,new String[]{MC+"server.WorldLoader$InitConfig",MC+"server.WorldLoader$WorldDataSupplier",MC+"server.WorldLoader$ResultFactory","java.util.concurrent.Executor","java.util.concurrent.Executor"},config,supplier,factory,direct,direct)).join();
            Path work=Files.createTempDirectory(output.toAbsolutePath().getParent(),"pending-native-world-");Files.writeString(work.resolve("server.properties"),"online-mode=false\nserver-ip=127.0.0.1\nserver-port=0\nenable-query=false\nenable-rcon=false\nmanagement-server-enabled=false\ntext-filtering-config=\nview-distance=2\nsimulation-distance=2\n");
            Object source=call(MC+"world.level.storage.LevelStorageSource","createDefault",null,new String[]{"java.nio.file.Path"},work);fni.c storage=(fni.c)call(MC+"world.level.storage.LevelStorageSource","createAccess",source,new String[]{"java.lang.String"},"world");
            asa settings=(asa)make(MC+"server.dedicated.DedicatedServerSettings",new String[]{"java.nio.file.Path"},work.resolve("server.properties"));
            Object fixer=call(MC+"util.datafix.DataFixers","getDataFixer",null,new String[]{});
            ano services=(ano)call(MC+"server.Services","create",null,new String[]{"com.mojang.authlib.yggdrasil.YggdrasilAuthenticationService","java.io.File"},YggdrasilAuthenticationService.createOffline(java.net.Proxy.NO_PROXY),work.toFile());
            ary nativeServer=new ary(Thread.currentThread(),storage,packs,stem,settings,(com.mojang.datafixers.DataFixer)fixer,services);nativeServer.a(new arx(nativeServer,stem.c(),storage.g()));
            Object stems=call(MC+"core.RegistryAccess","lookupOrThrow",nativeServer.bc(),new String[]{MC+"resources.ResourceKey"},field(MC+"core.registries.Registries","LEVEL_STEM"));
            Object nativeStem=call(MC+"core.Registry","getValueOrThrow",stems,new String[]{MC+"resources.ResourceKey"},field(MC+"world.level.dimension.LevelStem","OVERWORLD"));
            Object level=make(MC+"server.level.ServerLevel",new String[]{MC+"server.MinecraftServer","java.util.concurrent.Executor",MC+"world.level.storage.LevelStorageSource$LevelStorageAccess",MC+"world.level.storage.ServerLevelData",MC+"resources.ResourceKey",MC+"world.level.dimension.LevelStem","boolean","long","java.util.List","boolean",MC+"world.RandomSequences"},nativeServer,direct,storage,stem.d(),field(MC+"world.level.Level","OVERWORLD"),nativeStem,false,0L,List.of(),false,make(MC+"world.RandomSequences",new String[]{}));
            ((Map<Object,Object>)member(MC+"server.MinecraftServer","levels",nativeServer)).put(field(MC+"world.level.Level","OVERWORLD"),level);
            evidence.put("schema","native-worldgen-actor-transitions-v1");evidence.put("pins_verified",true);
            evidence.put("context",fullBoundary
                ? "genuine constructor-only native server, CARVERS chunks and WorldGenRegion FEATURES context, controlled pending placements followed by actual FULL request; no listeners, server loop or ticks"
                : "genuine constructor-only native server and CARVERS chunks, actual WorldGenRegion FEATURES step/cache/holders, no listeners, ticks, FULL promotion or SPAWN");
            capture(root,nativeServer,level);
            if(fullBoundary)evidence.put("full_boundary_context","actual native FULL request after controlled pending placements; no server loop or ticks");
        } catch(Throwable e){e.printStackTrace();evidence.put("failure",e.toString());}
        evidence.put("denied_network_attempts",deniedNetwork);
        var gson=new com.google.gson.GsonBuilder().serializeNulls().create();
        Files.writeString(output,gson.toJson(evidence));
        if(!evidence.containsKey("failure")) {
            Map<String,Object> consumer=new LinkedHashMap<>();
            consumer.put("schema","native-proto-template-actor-consumer-v1");
            consumer.put("server_sha1","64bb6d763bed0a9f1d632ec347938594144943ed");
            consumer.put("mappings_sha1","5621e9253f05fd57872bbe7f8ddf5f9a7d525955");
            consumer.put("inner_sha256","ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc");
            consumer.put("native_tick_count",evidence.get("native_tick_count"));
            consumer.put("records",evidence.get("proto_template_actors"));
            Files.writeString(output.resolveSibling(output.getFileName()+".proto-template-actors.json"),gson.toJson(consumer));
        }
        System.exit(evidence.containsKey("failure")?1:0);
    }
}
