package space.tiancai.hub;

import java.lang.reflect.*;
import java.util.*;
import kotlin.coroutines.Continuation;

/** Synthetic settings only: no IDE, network, PasswordSafe or user configuration. */
public final class BridgeContractTest {
    public static final class Settings {
        String base="https://old.example.invalid/v1",key="synthetic-before";
        boolean tools=false;
        boolean failKey=false;
        public String getProviderId(){return "OpenAILike";}
        public String getBaseUrl(){return base;}
        public void setBaseUrl(String value){base=value;}
        public boolean isToolEnabled(){return tools;}
        public void setToolEnabled(boolean value){tools=value;}
        public Object getApiKey(Continuation<?> ignored){return key;}
        public Object setApiKey(String value,Continuation<?> ignored){
            if(failKey)throw new IllegalStateException("Synthetic storage failure");
            key=value;return kotlin.Unit.INSTANCE;
        }
    }
    public static final class Enabled {
        boolean on=false;
        public boolean isProviderEnabled(String id){assert id.equals("OpenAILike");return on;}
        public void enableProvider(String id){on=true;}
        public void disableProvider(String id){on=false;}
    }
    @SuppressWarnings("unchecked")
    private static Map<String,Object> snapshot(Settings s,Enabled e)throws Exception {
        Method m=Bridge.class.getDeclaredMethod("snapshot",Object.class,Object.class);m.setAccessible(true);
        return (Map<String,Object>)m.invoke(null,s,e);
    }
    private static void apply(Settings s,Enabled e,Map<String,Object> v)throws Exception {
        Method m=Bridge.class.getDeclaredMethod("apply",Object.class,Object.class,Map.class);m.setAccessible(true);m.invoke(null,s,e,v);
    }
    private static void compensate(Settings s,Enabled e,Map<String,Object> before,Map<String,Object> after)throws Exception {
        Method m=Bridge.class.getDeclaredMethod("compensate",Object.class,Object.class,Map.class,Map.class);m.setAccessible(true);m.invoke(null,s,e,before,after);
    }
    public static void main(String[] args)throws Exception {
        Settings s=new Settings();Enabled e=new Enabled();Map<String,Object> before=snapshot(s,e);
        Map<String,Object> desired=Map.of("baseUrl","http://127.0.0.1:3210/ai-gateway/clients/webstorm-native/abcd/v1","apiKey","tch-synthetic","toolEnabled",true,"enabled",true);
        apply(s,e,desired);assert snapshot(s,e).equals(desired):"native write must be observable";
        compensate(s,e,before,desired);assert snapshot(s,e).equals(before):"exact undo";
        apply(s,e,desired);s.base="https://independent.example.invalid/v1";
        compensate(s,e,before,desired);
        assert s.base.equals("https://independent.example.invalid/v1"):"must protect a later external edit";
        assert s.key.equals("synthetic-before")&&!s.tools&&!e.on;
        s.base=before.get("baseUrl").toString();s.failKey=true;
        try{apply(s,e,desired);throw new AssertionError("expected storage failure");}catch(InvocationTargetException expected){}
        s.failKey=false;compensate(s,e,before,desired);assert snapshot(s,e).equals(before):"partial mutation must be compensated";
        System.out.println("WebStorm synthetic contracts: write/read, protected undo, independent edits, partial failure passed.");
    }
}
