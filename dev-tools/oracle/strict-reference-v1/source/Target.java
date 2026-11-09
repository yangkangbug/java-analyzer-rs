// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package strict;
// Unicode byte-position prefix: 🪴 中文.
public class Target {
 public void /*D:string_ref*/literal(String p) {}
 public void /*D:string_scalar*/literal(int p) {}
 public void /*D:parent*/pick(Parent p) {}
 public void /*D:scalar*/pick(int p) {}
 public void /*D:object*/specific(Object p) {}
 public void /*D:specific_scalar*/specific(int p) {}
 public void /*D:specific_parent*/specific(Parent p) {}
 public void /*D:reverse_parent*/reverse(Parent p) {}
 public void /*D:reverse_scalar*/reverse(int p) {}
 public void /*D:reverse_object*/reverse(Object p) {}
 public void /*D:mixed_ref*/mixed(Parent p, Parent q) {}
 public void /*D:mixed_scalar*/mixed(int p, Parent q) {}
 public void /*D:mixed_other*/mixed(Object p, int q) {}
 public void /*D:wrapper_object*/wrapper(Object p) {}
 public void /*D:wrapper_scalar*/wrapper(int p) {}
 public void /*D:loose_scalar*/loose(int p) {}
 public void /*D:array_parent*/array(Parent p) {}
 public void /*D:array_primitive*/array(int[] p) {}
 public void /*D:unknown_parent*/unknown(Parent p) {}
 public void /*D:unknown_scalar*/unknown(int p) {}
 public void /*D:static_parent*/stat(Parent p) {}
 public static void /*D:static_scalar*/stat(int p) {}
 private void /*D:private_scalar*/priv(int p) {}
 public void /*D:private_parent*/priv(Parent p) {}
 public <T> void /*D:generic_scalar*/generic(int p, T q) {}
 public void /*D:generic_parent*/generic(Parent p, Parent q) {}
 public void /*D:varargs_scalar*/varargs(int... p) {}
 public void /*D:varargs_parent*/varargs(Parent p) {}
}
