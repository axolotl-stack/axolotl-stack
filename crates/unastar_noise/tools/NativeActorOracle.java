/* Worldless Java 1.21.11 block-actor load/save observation adapter.
 * Usage: java -XX:ActiveProcessorCount=1 -Xmx1G -cp <bundled classpath>
 * NativeActorOracle.java <runtime-directory> <output-file>
 * No game entrypoint, level, network listener, or server is constructed.
 */
import java.io.*;
import java.lang.reflect.*;
import java.nio.file.*;
import java.security.*;
import java.util.*;
import java.util.zip.*;

public final class NativeActorOracle {
    static final String MC = "net.minecraft.";
    static final String TAG = MC + "nbt.CompoundTag", LIST = MC + "nbt.ListTag";
    static final String BE = MC + "world.level.block.entity.BlockEntity";
    static final String STATE = MC + "world.level.block.state.BlockState";
    static final String POS = MC + "core.BlockPos", PROVIDER = MC + "core.HolderLookup$Provider";
    static final String SERVER_SHA1 = "64bb6d763bed0a9f1d632ec347938594144943ed";
    static final String MAPPINGS_SHA1 = "5621e9253f05fd57872bbe7f8ddf5f9a7d525955";
    static final String CODE_SHA256 = "ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc";
    final Map<String,String> classes = new HashMap<>();
    final Map<String,Map<String,String>> members = new HashMap<>();
    Object provider, blockLookup, position;
    NativeActorOracle(Path mappings) throws Exception {
        String owner = null;
        for (String line : Files.readAllLines(mappings)) {
            if (line.isBlank() || line.startsWith("#")) continue;
            String[] p = line.trim().split(" -> ", 2);
            if (!Character.isWhitespace(line.charAt(0))) {
                owner = p[0]; classes.put(owner, p[1].substring(0,p[1].length()-1));
                members.put(owner,new HashMap<>());
            } else {
                String s = p[0].replaceFirst("^\\d+:\\d+:", "");
                int open = s.indexOf('('), end = open < 0 ? s.length() : s.indexOf(')')+1;
                String key = s.substring(s.lastIndexOf(' ',open < 0 ? s.length()-1 : open)+1,end);
                members.get(owner).putIfAbsent(key,p[1]);
            }
        }
    }
    Class<?> type(String n) throws Exception {
        return switch(n) { case "int" -> int.class; case "long" -> long.class;
            case "boolean" -> boolean.class; case "byte" -> byte.class;
            default -> Class.forName(classes.getOrDefault(n,n)); };
    }
    Object call(String owner,String name,Object receiver,String[] params,Object... args) throws Exception {
        Class<?>[] types = new Class<?>[params.length];
        for(int i=0;i<types.length;i++) types[i]=type(params[i]);
        String key=name+"("+String.join(",",params)+")";
        return type(owner).getMethod(Objects.requireNonNull(members.get(owner).get(key),owner+"."+key),types).invoke(receiver,args);
    }
    Object field(String owner,String name) throws Exception { return type(owner).getField(members.get(owner).get(name)).get(null); }
    Object tag() throws Exception { return type(TAG).getConstructor().newInstance(); }
    Object get(Object tag,String key) throws Exception { return call(TAG,"get",tag,new String[]{"java.lang.String"},key); }
    String string(Object tag,String key) throws Exception { return (String)call(TAG,"getStringOr",tag,new String[]{"java.lang.String","java.lang.String"},key,""); }
    byte[] bytes(Object tag) throws Exception {
        ByteArrayOutputStream out=new ByteArrayOutputStream();
        call(MC+"nbt.NbtIo","write",null,new String[]{TAG,"java.io.DataOutput"},tag,new DataOutputStream(out));
        return out.toByteArray();
    }
    Object save(Object tag,Object state) throws Exception {
        Object actor=call(BE,"loadStatic",null,new String[]{POS,STATE,TAG,PROVIDER},position,state,tag,provider);
        if(actor==null) throw new IllegalStateException("native loadStatic rejected "+tag);
        return saveActor(actor);
    }
    Object saveActor(Object actor) throws Exception {
        Object saved=call(BE,"saveWithFullMetadata",actor,new String[]{PROVIDER},provider);
        // LevelChunk's save boundary marks instantiated actors as unpacked.
        call(TAG,"putBoolean",saved,new String[]{"java.lang.String","boolean"},"keepPacked",false);
        return saved;
    }
    Object state(Object palette) throws Exception {
        return call(MC+"nbt.NbtUtils","readBlockState",null,new String[]{MC+"core.HolderGetter",TAG},blockLookup,palette);
    }
    void bootstrap() throws Exception {
        call(MC+"SharedConstants","tryDetectVersion",null,new String[]{});
        call(MC+"server.Bootstrap","bootStrap",null,new String[]{});
        provider=call(MC+"data.registries.VanillaRegistries","createLookup",null,new String[]{});
        blockLookup=call(PROVIDER,"lookupOrThrow",provider,new String[]{MC+"resources.ResourceKey"},field(MC+"core.registries.Registries","BLOCK"));
        position=type(POS).getConstructor(int.class,int.class,int.class).newInstance(0,0,0);
    }
    static void verify(Path path,String expected) throws Exception {
        MessageDigest digest=MessageDigest.getInstance(expected.length()==64?"SHA-256":"SHA-1");
        try(InputStream in=Files.newInputStream(path)) { byte[] b=new byte[65536]; int n; while((n=in.read(b))!=-1) digest.update(b,0,n); }
        String actual=HexFormat.of().formatHex(digest.digest());
        if(!actual.equals(expected)) throw new IllegalArgumentException(path+": unexpected digest "+actual);
    }
    static void record(DataOutputStream out,String key,byte[] before,byte[] after) throws Exception {
        byte[] k=key.getBytes(java.nio.charset.StandardCharsets.UTF_8);
        out.writeInt(k.length);out.write(k);out.writeInt(before.length);out.write(before);out.writeInt(after.length);out.write(after);
    }
    void run(Path runtime,Path output) throws Exception {
        bootstrap();
        Map<String,Integer> families=new TreeMap<>();int count=0, changed=0;
        try(DataOutputStream out=new DataOutputStream(Files.newOutputStream(output))) {
            out.writeBytes("NATJAVA2");out.writeUTF(SERVER_SHA1);out.writeUTF(MAPPINGS_SHA1);out.writeUTF(CODE_SHA256);
            // Minimal runtime-generated actor inputs, measured using the same load/save API.
            for(String[] pair:new String[][]{{"chest","chest"},{"trapped_chest","trapped_chest"},{"barrel","barrel"},{"dispenser","dispenser"},{"dropper","dropper"},{"spawner","mob_spawner"},{"trial_spawner","trial_spawner"},{"suspicious_sand","brushable_block"},{"suspicious_gravel","brushable_block"},{"decorated_pot","decorated_pot"}}) {
                Object input=tag(),palette=tag();
                call(TAG,"putString",input,new String[]{"java.lang.String","java.lang.String"},"id","minecraft:"+pair[1]);
                call(TAG,"putString",palette,new String[]{"java.lang.String","java.lang.String"},"Name","minecraft:"+pair[0]);
                Object actorState=state(palette);
                String factory=pair[1].equals("brushable_block")?"BRUSHABLE_BLOCK":pair[1].toUpperCase(Locale.ROOT);
                Object actor=call(MC+"world.level.block.entity.BlockEntityType","create",field(MC+"world.level.block.entity.BlockEntityType",factory),new String[]{POS,STATE},position,actorState);
                record(out,"runtime/"+pair[0],bytes(input),bytes(saveActor(actor)));
                if(pair[0].equals("spawner")) {
                    for(String mob:new String[]{"zombie","skeleton","spider","cave_spider","blaze","silverfish"}) {
                        Object fresh=call(MC+"world.level.block.entity.BlockEntityType","create",field(MC+"world.level.block.entity.BlockEntityType",factory),new String[]{POS,STATE},position,actorState);
                        Object random=call(MC+"util.RandomSource","create",null,new String[]{"long"},0L);
                        call(MC+"world.level.block.entity.SpawnerBlockEntity","setEntityId",fresh,new String[]{MC+"world.entity.EntityType",MC+"util.RandomSource"},field(MC+"world.entity.EntityType",mob.toUpperCase(Locale.ROOT)),random);
                        record(out,"runtime/spawner/"+mob,bytes(input),bytes(saveActor(fresh)));
                    }
                }
                for(long seed:new long[]{0L,Long.MIN_VALUE}) {
                    Object lootInput=tag();
                    call(TAG,"putString",lootInput,new String[]{"java.lang.String","java.lang.String"},"id","minecraft:"+pair[1]);
                    call(TAG,"putString",lootInput,new String[]{"java.lang.String","java.lang.String"},"LootTable","minecraft:chests/simple_dungeon");
                    call(TAG,"putLong",lootInput,new String[]{"java.lang.String","long"},"LootTableSeed",seed);
                    record(out,"evidence/loot/"+pair[0]+"/"+seed,bytes(lootInput),bytes(save(lootInput,actorState)));
                }
            }
            try(ZipFile jar=new ZipFile(runtime.resolve("versions/1.21.11/server-1.21.11.jar").toFile())) {
                List<? extends ZipEntry> entries=Collections.list(jar.entries()).stream().filter(e->e.getName().startsWith("data/minecraft/structure/")&&e.getName().endsWith(".nbt")).sorted(Comparator.comparing(ZipEntry::getName)).toList();
                for(ZipEntry entry:entries) {
                    Object root;
                    try(InputStream in=jar.getInputStream(entry)) { root=call(MC+"nbt.NbtIo","readCompressed",null,new String[]{"java.io.InputStream",MC+"nbt.NbtAccounter"},in,call(MC+"nbt.NbtAccounter","unlimitedHeap",null,new String[]{})); }
                    List<?> blocks=(List<?>)get(root,"blocks");
                    Object p=get(root,"palette");
                    List<?> palette=p==null?(List<?>)((List<?>)get(root,"palettes")).getFirst():(List<?>)p;
                    for(int i=0;i<blocks.size();i++) {
                        Object block=blocks.get(i),input=get(block,"nbt");if(input==null)continue;
                        String id=string(input,"id");
                        if(id.equals("minecraft:jigsaw") ||
                            (id.equals("minecraft:structure_block") && string(input,"mode").equals("DATA")))continue;
                        int index=(Integer)call(TAG,"getIntOr",block,new String[]{"java.lang.String","int"},"state",-1);
                        byte[] before=bytes(input),after=bytes(save(input,state(palette.get(index))));
                        record(out,entry.getName()+"#"+i,before,after);count++;if(!Arrays.equals(before,after))changed++;
                        families.merge(id,1,Integer::sum);
                    }
                }
            }
        }
        System.out.println("Native actor records="+count+", changed-by-save="+changed+", families="+families);
    }
    public static void main(String[] args) {
        try { Path runtime=Path.of(args[0]);verify(runtime.resolve("server.jar"),SERVER_SHA1);verify(runtime.resolve("server-mappings.txt"),MAPPINGS_SHA1);verify(runtime.resolve("versions/1.21.11/server-1.21.11.jar"),CODE_SHA256);
            new NativeActorOracle(runtime.resolve("server-mappings.txt")).run(runtime,Path.of(args[1]));System.exit(0);
        } catch(Throwable e) {e.printStackTrace();System.exit(1);}
    }
}
