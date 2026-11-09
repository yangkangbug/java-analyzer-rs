// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { static void /*D:float_index_candidate_one*/m(float a) {} static void /*D:float_index_candidate_two*/m(double a) {} static void use(float[] f,float i) { /*Q:float_index*/m(f[i]); } }
