/* Pinned 1.21.11 native WorldgenRandom reseed consumer fixture capture.
 * Uses actual official random methods only; no world/server initialization.
 * Compile with Gson on the classpath. Run with the pinned native server and
 * its dependency classpath: dripstoneprobe.NativeDripstoneReseedProbe <runtime> <output>.
 * The output is native expected data, not a hand-translated RNG implementation.
 */
package dripstoneprobe;

import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import com.google.gson.Gson;

public final class NativeDripstoneReseedProbe {
    static Object make(String owner,Class<?>[] types,Object...args)throws Exception {
        Constructor<?> c=Class.forName(owner).getDeclaredConstructor(types);c.setAccessible(true);return c.newInstance(args);
    }
    static Object call(Object rng,String name,Class<?>[] types,Object...args)throws Exception {
        Method m=rng.getClass().getMethod(name,types);m.setAccessible(true);return m.invoke(rng,args);
    }
    static Object field(Object receiver,String owner,String name)throws Exception {
        Field f=Class.forName(owner).getDeclaredField(name);f.setAccessible(true);return f.get(receiver);
    }
    static String bits(double value) { return Long.toUnsignedString(Double.doubleToRawLongBits(value)); }
    static Map<String,Object> gaussianCache(Object rng)throws Exception {
        Object cache=field(rng,"eur","i");
        return Map.of("present",field(cache,"eus","c"),"value_bits",bits((double)field(cache,"eus","b")));
    }
    static Object capture(String algorithm,long firstSeed,long secondSeed,boolean drain)throws Exception {
        Object rng=algorithm.equals("legacy")
            ?make("eur",new Class[]{long.class},firstSeed)
            :make("evp",new Class[]{Class.forName("bgr")},make(algorithm.equals("worldgen_legacy")?"eur":"evr",new Class[]{long.class},firstSeed));
        List<String> initial=new ArrayList<>();
        initial.add(bits((double)call(rng,"k",new Class[]{})));
        if(drain)initial.add(bits((double)call(rng,"k",new Class[]{})));
        Map<String,Object> before=gaussianCache(rng);
        call(rng,"b",new Class[]{long.class},secondSeed);
        Map<String,Object> after=gaussianCache(rng);
        String nextGaussian=bits((double)call(rng,"k",new Class[]{}));
        List<Integer> nextInts=new ArrayList<>();for(int i=0;i<4;i++)nextInts.add((int)call(rng,"f",new Class[]{}));
        return Map.of("algorithm",algorithm,"first_seed",firstSeed,"second_seed",secondSeed,"drain_before_reseed",drain,
            "initial_gaussian_bits",initial,"cache_before_reseed",before,"cache_after_reseed",after,
            "gaussian_after_reseed_bits",nextGaussian,"next_ints",nextInts);
    }
    static void pin(Path file,String algorithm,String expected)throws Exception {
        String actual=HexFormat.of().formatHex(java.security.MessageDigest.getInstance(algorithm).digest(Files.readAllBytes(file)));
        if(!actual.equals(expected))throw new SecurityException("native asset pin mismatch: "+file);
    }
    public static void main(String[] args)throws Exception {
        if(args.length!=2)throw new IllegalArgumentException("runtime-directory output-json");
        Path runtime=Path.of(args[0]);
        pin(runtime.resolve("server.jar"),"SHA-1","64bb6d763bed0a9f1d632ec347938594144943ed");
        pin(runtime.resolve("server-mappings.txt"),"SHA-1","5621e9253f05fd57872bbe7f8ddf5f9a7d525955");
        pin(runtime.resolve("versions/1.21.11/server-1.21.11.jar"),"SHA-256","ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc");
        List<String> deniedNetwork=new ArrayList<>();
        System.setSecurityManager(new SecurityManager() {
            public void checkPermission(java.security.Permission permission) {}
            void deny(String action) { deniedNetwork.add(action);throw new SecurityException("network forbidden: "+action); }
            public void checkConnect(String host,int port) { deny("connect "+host+":"+port); }
            public void checkListen(int port) { deny("listen "+port); }
            public void checkAccept(String host,int port) { deny("accept "+host+":"+port); }
        });
        List<Object> rows=new ArrayList<>();
        for(String algorithm:List.of("worldgen_xoroshiro","worldgen_legacy","legacy"))
            for(long[] seeds:new long[][]{{0L,1L},{987654321L,12345L},{Long.MAX_VALUE,Long.MIN_VALUE}})
                for(boolean drain:new boolean[]{false,true})rows.add(capture(algorithm,seeds[0],seeds[1],drain));
        Map<String,Object> output=new LinkedHashMap<>();
        output.put("version","1.21.11");
        output.put("profile","actual native wrapped Xoroshiro and Legacy reseed with inherited MarsagliaPolarGaussian; bare LegacyRandomSource reset control");
        output.put("server_sha1","64bb6d763bed0a9f1d632ec347938594144943ed");
        output.put("mappings_sha1","5621e9253f05fd57872bbe7f8ddf5f9a7d525955");
        output.put("inner_sha256","ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc");
        output.put("denied_network_attempts",deniedNetwork);output.put("server_constructed",false);output.put("cases",rows);
        Files.writeString(Path.of(args[1]),new Gson().toJson(output),StandardOpenOption.CREATE_NEW);
    }
}
