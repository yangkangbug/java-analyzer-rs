// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
class Bad { static void /*D:trailing_comma_candidate_one*/m(float a) {} static void /*D:trailing_comma_candidate_two*/m(double a) {} static void use(float f) { /*Q:trailing_comma*/m(f,); } }
