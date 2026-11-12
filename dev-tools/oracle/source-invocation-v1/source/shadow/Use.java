package shadow;
import api.Factory;
import api.Sink;
// Unicode 🧭 coordinate control
public class Use {
    static model.Product field;
    void run(Factory f, Sink s) {
        s./*Q:reference_actual*/accept(f.make());
        s./*Q:nested_actual*/accept(f.identity(f.make()));
        s./*Q:primitive_actual*/take(f.number());
        s./*Q:nested_primitive*/take(f.integer(f.number()));
        s./*Q:mixed_tree*/take(f.make().value());
        s./*Q:sibling_actuals*/pair(f.make(), f.identity(f.make()));
        f.make().next()./*Q:multi_receiver*/value();
        f.identity(f.make()).next()./*Q:actual_and_receiver*/value();
        f.make()./*Q:static_declared_control*/value();
        s./*Q:generic_refusal*/accept(f.generic());
        s./*Q:array_refusal*/accept(f.array()[0]);
        s./*Q:binary_refusal*/take(f.text().length());
        s./*Q:field_refusal*/accept(field);
        f.make().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next().next()./*Q:depth_refusal*/value();
    }
}
