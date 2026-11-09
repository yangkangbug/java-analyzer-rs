// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { static void /*D:array_invariance_candidate_one*/m(double[] a) {} static void /*D:array_invariance_candidate_two*/m(long[] a) {} static void use(float[] f) { /*Q:array_invariance*/m(f); } }
