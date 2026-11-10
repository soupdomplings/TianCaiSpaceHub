package space.tiancai.hub;

import com.intellij.openapi.project.Project;
import com.intellij.openapi.startup.ProjectActivity;
import kotlin.Unit;
import kotlin.coroutines.Continuation;

/** The only compile-time platform contracts; no IDE implementation is shipped. */
public final class Startup implements ProjectActivity {
    @Override public Object execute(Project project, Continuation<? super Unit> continuation) {
        Bridge.start();
        return Unit.INSTANCE;
    }
}
