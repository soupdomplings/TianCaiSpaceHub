package space.tiancai.hub;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.*;
import java.lang.reflect.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.nio.file.attribute.*;
import java.security.MessageDigest;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicReference;

/** Deliberately narrow local bridge; never reads other providers or project files. */
public final class Bridge {
    private static HttpServer server;
    private static String token;
    private static Object gson;
    private static Path registration;
    private static String instance;
    private static final String MANAGER="com.intellij.ml.llm.provider.openai.OpenAILikeLlmProviderSettingsManager";
    private static final String ENABLED="com.intellij.ml.llm.core.providers.ThirdPartyAIProvidersSettingsManager";
    private static final int MAX=262144;

    public static synchronized void start() {
        if(server!=null)return;
        try {
            gson=Class.forName("com.google.gson.Gson").getConstructor().newInstance();
            token=UUID.randomUUID()+"-"+UUID.randomUUID();instance=UUID.randomUUID().toString();
            server=HttpServer.create(new InetSocketAddress("127.0.0.1",0),8);
            server.setExecutor(Executors.newSingleThreadExecutor(r->{Thread t=new Thread(r,"TianCai model settings");t.setDaemon(true);return t;}));
            server.createContext("/v1/state",Bridge::handle);
            Path root=Path.of(System.getProperty("user.home"),".tiancaispace-hub","ide-bridge");
            safeDirectories(root);
            registration=root.resolve("webstorm-"+ProcessHandle.current().pid()+".json");
            Map<String,Object> info=new LinkedHashMap<>();info.put("version",1);info.put("instance",instance);info.put("pid",ProcessHandle.current().pid());info.put("port",server.getAddress().getPort());info.put("token",token);
            secureWrite(registration,json(info).getBytes(StandardCharsets.UTF_8));
            server.start();
            Runtime.getRuntime().addShutdownHook(new Thread(()->{
                try{server.stop(0);Files.deleteIfExists(registration);}catch(Exception ignored){}
            },"TianCai bridge shutdown"));
        } catch(Exception ignored) { if(server!=null)server.stop(0);server=null; }
    }

    @SuppressWarnings("unchecked") private static void handle(HttpExchange exchange) throws IOException {
        try {
            String auth=exchange.getRequestHeaders().getFirst("Authorization");
            if(exchange.getRequestHeaders().containsKey("Origin")||auth==null||!MessageDigest.isEqual(auth.getBytes(StandardCharsets.UTF_8),("Bearer "+token).getBytes(StandardCharsets.UTF_8))) {reply(exchange,403,Map.of("error","Local authorization required"));return;}
            if(!exchange.getRequestURI().toString().equals("/v1/state")){reply(exchange,404,Map.of("error","Unknown operation"));return;}
            Object manager=service(MANAGER);Object enabled=service(ENABLED);
            Map<String,Object> before=snapshot(manager,enabled);
            if(exchange.getRequestMethod().equals("GET")){reply(exchange,200,before);return;}
            if(!exchange.getRequestMethod().equals("POST")){reply(exchange,405,Map.of("error","Unsupported method"));return;}
            byte[] bytes=exchange.getRequestBody().readNBytes(MAX+1);if(bytes.length>MAX)throw new IllegalArgumentException();
            Map<String,Object> request=(Map<String,Object>)parse(new String(bytes,StandardCharsets.UTF_8));
            Map<String,Object> expected=(Map<String,Object>)request.get("expected"),desired=(Map<String,Object>)request.get("desired");
            if(!Objects.equals(expected,before)){reply(exchange,409,Map.of("error","IDE settings changed; reread before replacing"));return;}
            String base=Objects.toString(desired.get("baseUrl"),"");String key=Objects.toString(desired.get("apiKey"),"");
            // A restore may contain the previous user endpoint. Writes must be
            // either a local Hub credential or a CAS-protected exact restore.
            boolean restore=Boolean.TRUE.equals(request.get("restore"));
            if(!restore&&(!base.matches("http://127\\.0\\.0\\.1:[0-9]+/ai-gateway/clients/webstorm-native/[a-f0-9]+/v1")||!(key.startsWith("tch-")||key.isEmpty()&&!Boolean.TRUE.equals(desired.get("enabled")))))throw new IllegalArgumentException();
            try { apply(manager,enabled,desired);Map<String,Object> after=snapshot(manager,enabled);
                if(!sameSettings(desired,after))throw new IllegalStateException();reply(exchange,200,after);
            } catch(Exception failure) {
                // Compensate only the operation's own mutation. The bridge has
                // a single worker, and no other provider is ever modified.
                try{compensate(manager,enabled,before,desired);}catch(Exception ignored){}
                reply(exchange,502,Map.of("error","PasswordSafe or IDE settings could not be confirmed; reread state"));
            }
        } catch(Exception failure){reply(exchange,503,Map.of("error","AI Assistant interface unavailable or changed; use the supported IDE version and open a project"));}
        finally{exchange.close();}
    }
    private static Map<String,Object> snapshot(Object manager,Object enabled)throws Exception {
        String provider=(String)call(manager,"getProviderId");Map<String,Object> state=new LinkedHashMap<>();
        state.put("baseUrl",Objects.toString(call(manager,"getBaseUrl"),""));state.put("apiKey",Objects.toString(suspend(manager,"getApiKey"),""));state.put("toolEnabled",call(manager,"isToolEnabled"));state.put("enabled",call(enabled,"isProviderEnabled",provider));return state;
    }
    private static boolean sameSettings(Map<String,Object> expected,Map<String,Object> actual){
        for(String key:List.of("baseUrl","apiKey","toolEnabled","enabled"))if(!Objects.equals(expected.get(key),actual.get(key)))return false;return true;
    }
    private static void apply(Object manager,Object enabled,Map<String,Object> state)throws Exception {
        call(manager,"setBaseUrl",Objects.toString(state.get("baseUrl"),""));call(manager,"setToolEnabled",Boolean.TRUE.equals(state.get("toolEnabled")));
        suspend(manager,"setApiKey",Objects.toString(state.get("apiKey"),""));
        call(enabled,Boolean.TRUE.equals(state.get("enabled"))?"enableProvider":"disableProvider",call(manager,"getProviderId"));
    }
    private static void compensate(Object manager,Object enabled,Map<String,Object> before,Map<String,Object> desired)throws Exception {
        // IDE settings can also be edited outside this bridge. Restore only
        // fields still carrying this operation's value, never a third value.
        if(Objects.equals(call(manager,"getBaseUrl"),desired.get("baseUrl")))call(manager,"setBaseUrl",before.get("baseUrl"));
        if(Objects.equals(call(manager,"isToolEnabled"),desired.get("toolEnabled")))call(manager,"setToolEnabled",before.get("toolEnabled"));
        if(Objects.equals(Objects.toString(suspend(manager,"getApiKey"),""),desired.get("apiKey")))suspend(manager,"setApiKey",before.get("apiKey"));
        Object provider=call(manager,"getProviderId");if(Objects.equals(call(enabled,"isProviderEnabled",provider),desired.get("enabled")))call(enabled,Boolean.TRUE.equals(before.get("enabled"))?"enableProvider":"disableProvider",provider);
    }
    private static Object service(String name)throws Exception {
        Object app=Class.forName("com.intellij.openapi.application.ApplicationManager").getMethod("getApplication").invoke(null);
        return call(app,"getService",Class.forName(name));
    }
    private static Object call(Object owner,String name,Object...args)throws Exception {
        for(Method method:owner.getClass().getMethods())if(method.getName().equals(name)&&method.getParameterCount()==args.length){try{return method.invoke(owner,args);}catch(IllegalArgumentException incompatible){}}
        throw new NoSuchMethodException(name);
    }
    private static Object suspend(Object owner,String method,Object...args)throws Exception {
        Class<?> continuation=Class.forName("kotlin.coroutines.Continuation");CountDownLatch latch=new CountDownLatch(1);AtomicReference<Object> completed=new AtomicReference<>();
        Object callback=java.lang.reflect.Proxy.newProxyInstance(Bridge.class.getClassLoader(),new Class<?>[]{continuation},(p,m,a)->{
            if(m.getName().equals("getContext"))return Class.forName("kotlin.coroutines.EmptyCoroutineContext").getField("INSTANCE").get(null);
            if(m.getName().equals("resumeWith")){completed.set(a[0]);latch.countDown();return null;}return null;
        });
        Object[] parameters=Arrays.copyOf(args,args.length+1);parameters[args.length]=callback;Object value=call(owner,method,parameters);
        Object suspended=Class.forName("kotlin.coroutines.intrinsics.IntrinsicsKt").getMethod("getCOROUTINE_SUSPENDED").invoke(null);
        if(value==suspended){if(!latch.await(20,TimeUnit.SECONDS))throw new TimeoutException();value=completed.get();}
        if(value!=null&&value.getClass().getName().equals("kotlin.Result$Failure"))throw new IllegalStateException("PasswordSafe operation failed");return value;
    }
    private static String json(Object value)throws Exception{return (String)gson.getClass().getMethod("toJson",Object.class).invoke(gson,value);}
    private static Object parse(String value)throws Exception{return gson.getClass().getMethod("fromJson",String.class,Class.class).invoke(gson,value,Object.class);}
    private static void reply(HttpExchange exchange,int status,Object value)throws IOException {
        byte[] bytes;try{bytes=json(value).getBytes(StandardCharsets.UTF_8);}catch(Exception failure){bytes="{}".getBytes(StandardCharsets.UTF_8);}
        exchange.getResponseHeaders().set("Content-Type","application/json");exchange.getResponseHeaders().set("Cache-Control","no-store");exchange.sendResponseHeaders(status,bytes.length);exchange.getResponseBody().write(bytes);
    }
    private static void safeDirectories(Path path)throws IOException {
        for(Path parent=path;parent!=null;parent=parent.getParent())if(Files.isSymbolicLink(parent))throw new IOException("Linked directory");Files.createDirectories(path);
    }
    private static void secureWrite(Path path,byte[] value)throws IOException {
        if(Files.isSymbolicLink(path))throw new IOException("Linked registration");
        try{Files.createFile(path,PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rw-------")));}
        catch(UnsupportedOperationException windows){Files.createFile(path);}
        catch(FileAlreadyExistsException existing){Files.delete(path);secureWrite(path,value);return;}
        AclFileAttributeView acl=Files.getFileAttributeView(path,AclFileAttributeView.class);
        if(acl!=null){UserPrincipal owner=Files.getOwner(path);acl.setAcl(List.of(AclEntry.newBuilder().setType(AclEntryType.ALLOW).setPrincipal(owner).setPermissions(EnumSet.allOf(AclEntryPermission.class)).build()));}
        Files.write(path,value,StandardOpenOption.TRUNCATE_EXISTING);
    }
}
