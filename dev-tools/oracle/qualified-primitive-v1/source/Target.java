// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package qualified;
public class Target {
 public void /*D:pick_int*/pick(int x) {}
 public void /*D:pick_long*/pick(long x) {}
 public void /*D:reverse_long*/reverse(long x) {}
 public void /*D:reverse_int*/reverse(int x) {}
 public void /*D:widen_int*/widen(int x) {}
 public void /*D:widen_short*/widen(short x) {}
 public void /*D:chars_int*/chars(int x) {}
 public void /*D:chars_char*/chars(char x) {}
 public void /*D:bool_int*/bool(int x) {}
 public void /*D:bool_boolean*/bool(boolean x) {}
 public void /*D:boxed*/boxed(Integer x) {}
 public void /*D:varargs*/varargs(int... x) {}
 public <T> void /*D:generic_ref*/generic(T x) {}
 public void /*D:generic_int*/generic(int x) {}
 public void /*D:mixed_int*/mixed(int x, Object y) {}
 public void /*D:mixed_long*/mixed(long x, Object y) {}
}
