package casts;
// Unicode coordinate sentinel: 🧭
public class Use {
    void run(Target t) {
        t./*Q:typed_parent*/pick((Parent) null);
        t./*Q:plain_null*/pick(null);
        t./*Q:typed_child*/pick((Child) null);
        t./*Q:parenthesized*/pick(((Parent) ((null))));
        t./*Q:array_refusal*/array((Parent[]) null);
        t./*Q:generic_refusal*/generic((Box<String>) null);
        t./*Q:nested_refusal*/pick((Parent) (Child) null);
        t./*Q:call_refusal*/pick((Parent) t.make());
        t./*Q:depth_refusal*/pick((Parent) ((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((null)))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))));
        t./*Q:string_competitor*/text((String) null);
    }
}
