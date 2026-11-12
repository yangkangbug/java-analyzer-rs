// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package mixed;
public class Target {
 public void /*D:mix_int*/mix(int n, Sub r) {}
 public void /*D:mix_long*/mix(long n, Ref r) {}
 public void /*D:swap_ref*/swap(Sub r, int n) {}
 public void /*D:swap_obj*/swap(Ref r, long n) {}
 public void /*D:multi_int*/multi(int a, int b) {}
 public void /*D:multi_long*/multi(long a, long b) {}
 public void /*D:reverse_long*/reverse(long a, Ref b) {}
 public void /*D:reverse_int*/reverse(int a, Sub b) {}
 public void /*D:tri_int*/tri(int a, Sub b, short c) {}
 public void /*D:tri_long*/tri(long a, Ref b, int c) {}
 public void /*D:set*/set(int a, Ref b) {}
 public void /*D:boxed*/boxed(Integer a, Ref b) {}
 public void /*D:varargs*/varargs(Ref r, int... a) {}
}
