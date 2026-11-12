package client;
public class Use {
    void run(Child c, model.Value external, Value local) {
        c./*Q:declaring_import*/accept(external);
        c./*Q:child_shadow*/accept(local);
    }
}
