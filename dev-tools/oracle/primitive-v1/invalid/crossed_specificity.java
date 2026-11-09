// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { static void /*D:crossed_specificity_candidate_one*/m(float a,double b) {} static void /*D:crossed_specificity_candidate_two*/m(double a,float b) {} static void use(float f) { /*Q:crossed_specificity*/m(f,f); } }
