package oracle;
// Astral offset guard: 🧭. These programs are analyzed, never executed.
class Containment {
  int /*D:outer_field*/collision;
  void first(int /*D:outer_parameter*/parameter) {
    int /*D:before_local*/before = 1;
    int a = /*Q:before_type*/before;
    class /*D:local_type*/Hidden<T> {
      int /*D:hidden_field*/collision;
      int /*D:hidden_only*/hiddenOnly;
      void member(int /*D:hidden_parameter*/parameter) {
        int x = /*Q:inside_field*/collision;
        int y = /*Q:inside_parameter*/parameter;
        int z = /*Q:inside_capture*/before;
      }
    }
    int b = /*Q:after_type*/before;
    int c = /*Q:after_parameter*/parameter;
    int d = /*Q:before_shadow*/collision;
    int /*D:after_local*/collision = 2;
    int e = /*Q:after_shadow*/collision;
    {
      int /*D:block_local*/nested = 3;
      class /*D:nested_type*/Nested { int before; }
      int f = /*Q:after_nested*/nested;
      int g = /*Q:outer_in_block*/before;
    }
    /*Q:local_type_use*/Hidden<String> value = null;
    int h = /*Q:after_type_use*/before;
  }
  void /*D:later_method*/later(int /*D:later_parameter*/parameter) {
    int a = /*Q:sibling_field*/collision;
    int b = /*Q:sibling_parameter*/parameter;
  }
  void caller() { /*Q:later_call*/later(1); }
}
